//! td/mtproto/RSA.cpp on the Rust side: PEM/SPKI parsing, the fingerprint, and
//! the two raw x^e mod n operations.
//!
//! ```text
//! rsa_probe <pem_file> <load|fingerprint|encrypt|decrypt> [hex_256_bytes]
//! ```
//!
//! `encrypt` and `decrypt_signature` are plain modular exponentiation (TDLib
//! does not do OAEP here), so both sides must agree byte for byte. The three
//! failure messages come from the OpenSSL path in from_pem_public_key().

use std::env;
use std::fs;
use std::process::ExitCode;

use num_bigint::BigUint;
use sha1::{Digest, Sha1};

const RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const READ_ERROR: &str = "Error while reading RSA public key";
const NOT_RSA_ERROR: &str = "Key is not an RSA key";
const SIZE_ERROR: &str = "EVP_PKEY_size != 256";

fn unhex(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index + 1 < bytes.len() {
        let high = (bytes[index] as char).to_digit(16).unwrap_or(0) as u8;
        let low = (bytes[index + 1] as char).to_digit(16).unwrap_or(0) as u8;
        out.push((high << 4) | low);
        index += 2;
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

fn base64_decode(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    let mut seen_padding = false;
    for &c in text {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                seen_padding = true;
                continue;
            }
            b'\r' | b'\n' | b' ' | b'\t' => continue,
            _ => return None,
        };
        if seen_padding {
            return None;
        }
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

fn pem_body(pem: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(pem).ok()?;
    let begin = "-----BEGIN PUBLIC KEY-----";
    let end = "-----END PUBLIC KEY-----";
    let start = text.find(begin)? + begin.len();
    let stop = start + text[start..].find(end)?;
    base64_decode(text[start..stop].as_bytes())
}

struct Der<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Der<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn at_end(&self) -> bool {
        self.pos == self.data.len()
    }
    fn byte(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(byte)
    }
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        if self.pos + count > self.data.len() {
            return None;
        }
        let slice = &self.data[self.pos..self.pos + count];
        self.pos += count;
        Some(slice)
    }
    fn length(&mut self) -> Option<usize> {
        let first = self.byte()?;
        if first < 0x80 {
            return Some(usize::from(first));
        }
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 {
            return None;
        }
        let mut value = 0usize;
        for &byte in self.take(count)? {
            value = (value << 8) | usize::from(byte);
        }
        Some(value)
    }
    fn tlv(&mut self, tag: u8) -> Option<&'a [u8]> {
        if self.byte()? != tag {
            return None;
        }
        let length = self.length()?;
        self.take(length)
    }
}

fn parse_public_key(der: &[u8]) -> Result<(BigUint, BigUint), String> {
    let mut top = Der::new(der);
    let body = top.tlv(0x30).ok_or(READ_ERROR)?;
    if !top.at_end() {
        return Err(READ_ERROR.to_string());
    }
    let mut outer = Der::new(body);
    let algorithm = outer.tlv(0x30).ok_or(READ_ERROR)?;
    let bit_string = outer.tlv(0x03).ok_or(READ_ERROR)?;
    if !outer.at_end() {
        return Err(READ_ERROR.to_string());
    }
    let mut algorithm_reader = Der::new(algorithm);
    let oid = algorithm_reader.tlv(0x06).ok_or(READ_ERROR)?;
    if oid != RSA_OID {
        return Err(NOT_RSA_ERROR.to_string());
    }
    if bit_string.is_empty() || bit_string[0] != 0 {
        return Err(READ_ERROR.to_string());
    }
    let mut rsa_reader = Der::new(&bit_string[1..]);
    let sequence = rsa_reader.tlv(0x30).ok_or(READ_ERROR)?;
    let mut inner = Der::new(sequence);
    let n = inner.tlv(0x02).ok_or(READ_ERROR)?;
    let e = inner.tlv(0x02).ok_or(READ_ERROR)?;
    Ok((BigUint::from_bytes_be(n), BigUint::from_bytes_be(e)))
}

fn tl_string(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 8);
    if data.len() < 254 {
        out.push(data.len() as u8);
    } else {
        out.push(0xfe);
        out.push((data.len() & 0xff) as u8);
        out.push(((data.len() >> 8) & 0xff) as u8);
        out.push(((data.len() >> 16) & 0xff) as u8);
    }
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

fn fingerprint(n: &BigUint, e: &BigUint) -> i64 {
    let mut data = Vec::new();
    // tl_store_unsafe() on a concrete class writes the fields only; the
    // constructor id belongs to the boxed wrapper (same rule as td_api).
    data.extend_from_slice(&tl_string(&n.to_bytes_be()));
    data.extend_from_slice(&tl_string(&e.to_bytes_be()));
    let digest = Sha1::digest(&data);
    i64::from_le_bytes(digest[12..20].try_into().expect("eight bytes"))
}

fn to_be_256(value: &BigUint) -> Vec<u8> {
    let bytes = value.to_bytes_be();
    let mut out = vec![0u8; 256];
    let start = 256 - bytes.len();
    out[start..].copy_from_slice(&bytes);
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: rsa_probe <pem_file> <load|fingerprint|encrypt|decrypt> [hex]");
        return ExitCode::from(2);
    }
    let pem = match fs::read(&args[1]) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("read {}: {error}", args[1]);
            return ExitCode::from(1);
        }
    };
    let der = match pem_body(&pem) {
        Some(der) => der,
        None => {
            println!("message={READ_ERROR}");
            return ExitCode::SUCCESS;
        }
    };
    let (n, e) = match parse_public_key(&der) {
        Ok(pair) => pair,
        Err(message) => {
            println!("message={message}");
            return ExitCode::SUCCESS;
        }
    };
    if (n.bits() + 7) / 8 != 256 {
        println!("message={SIZE_ERROR}");
        return ExitCode::SUCCESS;
    }

    match args[2].as_str() {
        "load" => println!("ok=1"),
        "fingerprint" => println!("ok=1 fingerprint={} size=256", fingerprint(&n, &e)),
        mode @ ("encrypt" | "decrypt") => {
            if args.len() < 4 {
                eprintln!("{mode} needs a hex payload");
                return ExitCode::from(2);
            }
            let from = unhex(&args[3]);
            if from.len() != 256 {
                eprintln!("payload must be 256 bytes, got {}", from.len());
                return ExitCode::from(2);
            }
            let x = BigUint::from_bytes_be(&from);
            if mode == "encrypt" && x >= n {
                println!("ok=0");
                return ExitCode::SUCCESS;
            }
            let y = x.modpow(&e, &n);
            if mode == "encrypt" {
                println!("ok=1");
            }
            println!("out={}", hex_encode(&to_be_256(&y)));
        }
        other => {
            eprintln!("unknown mode {other}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}