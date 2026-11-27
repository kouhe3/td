//! Read an MTProto 2.0 packet the way a client writes it (X = 0).
//!
//! TDLib's `Transport::write` always emits client-direction packets (X = 0)
//! while `Transport::read` is hard-wired to X = 8 (client reading the server),
//! so the two cannot be fed into each other. The oracle here is therefore
//! TDLib's writer, and this probe is the reader on the other side.
//!
//! Packet layout: auth_key_id(8) msg_key(16)
//!   encrypted: salt(8) session_id(8) msg_id(8) seq_no(4) length(4) payload padding
//!
//! Usage: transport_probe <fixture.json> <packet_hex>
//! Output: payload=, salt=, session_id=, msg_id=, seq_no=, msg_key_ok=

use std::env;
use std::fs;
use std::process::ExitCode;

use aes::cipher::{BlockDecrypt, KeyInit};
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

fn kdf2(auth_key: &[u8], msg_key: &[u8], x: usize) -> ([u8; 32], [u8; 32]) {
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
    (aes_key, aes_iv)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: transport_probe <fixture.json> <packet_hex>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("read {}: {e}", args[1]);
            return ExitCode::from(1);
        }
    };
    let auth_key = hex_decode(json_field(&text, "auth_key").unwrap_or_else(|| panic!("missing auth_key")));
    let packet = hex_decode(&args[2]);
    if packet.len() < 32 {
        eprintln!("packet too small");
        return ExitCode::from(1);
    }

    let msg_key = &packet[8..24];
    let mut encrypted = packet[24..].to_vec();
    // The last partial block is not encrypted (TDLib drops it on read too).
    encrypted.truncate(encrypted.len() & !15);

    let (aes_key, aes_iv) = kdf2(&auth_key, msg_key, 0);
    let plain = aes_ige_decrypt(&aes_key, &aes_iv, &encrypted);

    let recomputed = sha256(&[&auth_key[88..120], &plain]);
    let msg_key_ok = &recomputed[8..24] == msg_key;

    let salt = u64::from_le_bytes(plain[0..8].try_into().unwrap());
    let session_id = u64::from_le_bytes(plain[8..16].try_into().unwrap());
    let msg_id = u64::from_le_bytes(plain[16..24].try_into().unwrap());
    let seq_no = u32::from_le_bytes(plain[24..28].try_into().unwrap()) as i32;
    let len = u32::from_le_bytes(plain[28..32].try_into().unwrap()) as usize;

    println!("payload={}", hex_encode(&plain[32..32 + len]));
    println!("salt={salt}");
    println!("session_id={session_id}");
    println!("msg_id={msg_id}");
    println!("seq_no={seq_no}");
    println!("msg_key_ok={msg_key_ok}");
    ExitCode::SUCCESS
}