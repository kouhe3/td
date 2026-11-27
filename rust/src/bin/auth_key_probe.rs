//! Reproduce steps 1-4 of Telegram's auth-key sample.
//!
//! Input: the `resPQ` body from
//! https://core.telegram.org/mtproto/samples-auth_key together with the fixed
//! nonce / server_nonce / new_nonce / dc that the sample pins.
//! Output: the quadratic-factor decomposition of `pq` and the TL serialization
//! of `p_q_inner_data_dc#955ff5a9`, both of which the sample states explicitly.
//!
//! TL strings are a one-byte length (or 0xFE and three bytes) followed by the
//! bytes and zero padding up to a 4-byte boundary.
//!
//! Usage: auth_key_probe <fixture.json>
//! Output (stdout, one key per line):
//!   pq=<decimal>
//!   p=<decimal>
//!   q=<decimal>
//!   p_q_inner_data=<hex>

use std::env;
use std::fs;
use std::process::ExitCode;

use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes256;
use sha1::{Digest, Sha1};

/// The sample prints the TL bytes `95 5F F5 A9`; TL numbers are little-endian.
const P_Q_INNER_DATA_DC: u32 = 0xa9f5_5f95;

fn hex_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let val = |c: u8| -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("bad hex digit"),
        }
    };
    let mut i = 0;
    while i + 1 < bytes.len() {
        out.push((val(bytes[i]) << 4) | val(bytes[i + 1]));
        i += 2;
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

// --- A tiny JSON reader: the fixture is a flat object of strings and numbers. ---

fn json_field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\"");
    let start = text.find(&key)? + key.len();
    let rest = &text[start..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(&stripped[..end])
    } else {
        let end = rest
            .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
            .unwrap_or(rest.len());
        Some(rest[..end].trim())
    }
}

// --- TL string ---

fn tl_string(bytes: &[u8]) -> Vec<u8> {
    assert!(bytes.len() < 254, "long TL strings are not needed here");
    let mut out = Vec::with_capacity(1 + bytes.len() + 3);
    out.push(bytes.len() as u8);
    out.extend_from_slice(bytes);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// Read a TL string: one-byte length (0xFE and three bytes for long ones),
/// the data, and zero padding up to a 4-byte boundary.
fn tl_read_string<'a>(buf: &'a [u8], off: &mut usize) -> &'a [u8] {
    let first = buf[*off];
    let (len, hdr) = if first < 254 {
        (first as usize, 1)
    } else {
        (
            buf[*off + 1] as usize | (buf[*off + 2] as usize) << 8 | (buf[*off + 3] as usize) << 16,
            4,
        )
    };
    let start = *off + hdr;
    let end = start + len;
    *off = end + (4 - (hdr + len) % 4) % 4;
    &buf[start..end]
}

/// AES-256-IGE decryption: P_i = D(C_i ^ P_{i-1}) ^ C_{i-1}, seeded with
/// C_{-1} = iv[0..16] and P_{-1} = iv[16..32].
fn aes_ige_decrypt(key: &[u8], iv: &[u8], input: &[u8]) -> Vec<u8> {
    let cipher = Aes256::new_from_slice(key).expect("AES-256 key");
    let mut out = Vec::with_capacity(input.len());
    let mut prev_c = iv[0..16].to_vec();
    let mut prev_p = iv[16..32].to_vec();
    for chunk in input.chunks(16) {
        let mut block = aes::cipher::generic_array::GenericArray::clone_from_slice(chunk);
        for i in 0..16 {
            block[i] ^= prev_p[i];
        }
        cipher.decrypt_block(&mut block);
        let mut plain = [0u8; 16];
        for i in 0..16 {
            plain[i] = block[i] ^ prev_c[i];
        }
        out.extend_from_slice(&plain);
        prev_c = chunk.to_vec();
        prev_p = plain.to_vec();
    }
    out
}

// --- pq factorization (u128 arithmetic, deterministic Miller-Rabin) ---

fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    ((a as u128 * b as u128) % m as u128) as u64
}

fn pow_mod(mut base: u64, mut exp: u64, m: u64) -> u64 {
    let mut acc = 1u64;
    base %= m;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = mul_mod(acc, base, m);
        }
        base = mul_mod(base, base, m);
        exp >>= 1;
    }
    acc
}

fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    for p in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n % p == 0 {
            return n == p;
        }
    }
    let mut d = n - 1;
    let mut s = 0;
    while d & 1 == 0 {
        d >>= 1;
        s += 1;
    }
    // Deterministic for n < 2^64 with these bases.
    for a in [2u64, 325, 9375, 28178, 450775, 9780504, 1795265022] {
        if a % n == 0 {
            continue;
        }
        let mut x = pow_mod(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        let mut composite = true;
        for _ in 0..s - 1 {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                composite = false;
                break;
            }
        }
        if composite {
            return false;
        }
    }
    true
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Pollard-Brent rho; the sample's pq factors quickly.
fn rho(n: u64) -> u64 {
    if n % 2 == 0 {
        return 2;
    }
    let mut c = 1u64;
    loop {
        let f = |x: u64| (mul_mod(x, x, n) + c) % n;
        let (mut x, mut y, mut d) = (2u64, 2u64, 1u64);
        while d == 1 {
            x = f(x);
            y = f(f(y));
            d = gcd(x.abs_diff(y), n);
        }
        if d != n {
            return d;
        }
        c += 1;
    }
}

fn factor(n: u64) -> (u64, u64) {
    let d = if is_prime(n) { n } else { rho(n) };
    let mut a = d;
    let mut b = n / d;
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    (a, b)
}

/// `pq` is serialized as the big-endian bytes with no leading zero bytes.
fn big_endian(value: u64) -> Vec<u8> {
    let mut out = value.to_be_bytes().to_vec();
    while out.len() > 1 && out[0] == 0 {
        out.remove(0);
    }
    out
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: auth_key_probe <fixture.json>");
        return ExitCode::from(2);
    };
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("read {path}: {e}");
            return ExitCode::from(1);
        }
    };
    let get = |name: &str| json_field(&text, name).unwrap_or_else(|| panic!("missing {name}"));

    // resPQ body: constructor, nonce, server_nonce, pq string, fingerprints vector.
    let body = hex_decode(get("res_pq_body"));
    let constructor = u32::from_le_bytes(body[0..4].try_into().unwrap());
    assert_eq!(constructor, 0x05162463, "resPQ constructor");
    let pq_len = body[36] as usize;
    let pq = body[37..37 + pq_len]
        .iter()
        .fold(0u64, |acc, &b| (acc << 8) | b as u64);

    let (p, q) = factor(pq);

    let mut data = Vec::new();
    data.extend_from_slice(&P_Q_INNER_DATA_DC.to_le_bytes());
    data.extend_from_slice(&tl_string(&big_endian(pq)));
    data.extend_from_slice(&tl_string(&big_endian(p)));
    data.extend_from_slice(&tl_string(&big_endian(q)));
    data.extend_from_slice(&hex_decode(get("nonce")));
    data.extend_from_slice(&hex_decode(get("server_nonce")));
    data.extend_from_slice(&hex_decode(get("new_nonce")));
    data.extend_from_slice(&(get("dc").parse::<i32>().unwrap()).to_le_bytes());

    println!("pq={pq}");
    println!("p={p}");
    println!("q={q}");
    println!("p_q_inner_data={}", hex_encode(&data));

    // Steps 5-6: decrypt the server's DH answer and parse server_DH_inner_data.
    let decrypted = aes_ige_decrypt(
        &hex_decode(get("tmp_aes_key")),
        &hex_decode(get("tmp_aes_iv")),
        &hex_decode(get("encrypted_answer")),
    );
    let answer = &decrypted[20..];

    let mut off = 4; // server_DH_inner_data constructor
    off += 16 + 16; // nonce, server_nonce
    let g = i32::from_le_bytes(answer[off..off + 4].try_into().unwrap());
    off += 4;
    let dh_prime = tl_read_string(answer, &mut off);
    let g_a = tl_read_string(answer, &mut off);
    let server_time = i32::from_le_bytes(answer[off..off + 4].try_into().unwrap());
    off += 4;
    // answer_with_hash = SHA1(answer) || answer || zero padding to a 16-byte
    // boundary, so the hash covers only the parsed object, not the padding.
    let answer_len = off;
    let digest = Sha1::digest(&answer[..answer_len]);

    println!("answer_sha1={}", hex_encode(&decrypted[..20]));
    println!("answer_len={answer_len}");
    println!("hash_ok={}", digest.as_slice() == &decrypted[..20]);
    println!("g={g}");
    println!("server_time={server_time}");
    println!("dh_prime_len={}", dh_prime.len());
    println!("g_a_len={}", g_a.len());
    ExitCode::SUCCESS
}