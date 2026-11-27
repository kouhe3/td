//! MTProto 2.0 message framing: message key, KDF2 and AES-IGE encryption.
//!
//! Oracle: tools/fixtures/msgprobe.cpp, built against TDLib's own
//! `mtproto::KDF2` and `aes_ige_encrypt`; its output is pinned in
//! tools/fixtures/auth_key_sample.json under `msg_layer`.
//!
//! msg_key_large = SHA256(auth_key[88+X .. +32] || plaintext)
//! msg_key       = msg_key_large[8..24]
//! sha256_a      = SHA256(msg_key || auth_key[X .. +36])
//! sha256_b      = SHA256(auth_key[40+X .. +36] || msg_key)
//! aes_key       = a[0..8] || b[8..24] || a[24..32]
//! aes_iv        = b[0..8] || a[8..24] || b[24..32]
//! ciphertext    = AES-256-IGE(aes_key, aes_iv, plaintext)
//!
//! Usage: msg_layer_probe <fixture.json>
//! Output: msg_key=, aes_key=, aes_iv=, ciphertext= (hex)

use std::env;
use std::fs;
use std::process::ExitCode;

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes256;
use sha2::{Digest, Sha256};

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

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn aes_ige_encrypt(key: &[u8], iv: &[u8], input: &[u8]) -> Vec<u8> {
    let cipher = Aes256::new_from_slice(key).expect("AES-256 key");
    let mut out = Vec::with_capacity(input.len());
    let mut prev_c = iv[0..16].to_vec();
    let mut prev_p = iv[16..32].to_vec();
    for chunk in input.chunks(16) {
        let mut block = aes::cipher::generic_array::GenericArray::clone_from_slice(chunk);
        for i in 0..16 {
            block[i] ^= prev_c[i];
        }
        cipher.encrypt_block(&mut block);
        let mut cipher_block = [0u8; 16];
        for i in 0..16 {
            cipher_block[i] = block[i] ^ prev_p[i];
        }
        out.extend_from_slice(&cipher_block);
        prev_c = cipher_block.to_vec();
        prev_p = chunk.to_vec();
    }
    out
}

/// Kept for symmetry with the decryption path used by the DH lane.
#[allow(dead_code)]
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

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: msg_layer_probe <fixture.json>");
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

    let auth_key = hex_decode(get("auth_key"));
    let plaintext = hex_decode(get("plaintext"));
    let x: usize = get("x").parse().unwrap();

    let msg_key_large = sha256(&[&auth_key[88 + x..88 + x + 32], &plaintext]);
    let msg_key = &msg_key_large[8..24];

    let sha_a = sha256(&[msg_key, &auth_key[x..x + 36]]);
    let sha_b = sha256(&[&auth_key[40 + x..40 + x + 36], msg_key]);

    let mut aes_key = [0u8; 32];
    aes_key[..8].copy_from_slice(&sha_a[..8]);
    aes_key[8..24].copy_from_slice(&sha_b[8..24]);
    aes_key[24..].copy_from_slice(&sha_a[24..32]);

    let mut aes_iv = [0u8; 32];
    aes_iv[..8].copy_from_slice(&sha_b[..8]);
    aes_iv[8..24].copy_from_slice(&sha_a[8..24]);
    aes_iv[24..].copy_from_slice(&sha_b[24..32]);

    let ciphertext = aes_ige_encrypt(&aes_key, &aes_iv, &plaintext);

    println!("msg_key={}", hex_encode(msg_key));
    println!("aes_key={}", hex_encode(&aes_key));
    println!("aes_iv={}", hex_encode(&aes_iv));
    println!("ciphertext={}", hex_encode(&ciphertext));
    ExitCode::SUCCESS
}