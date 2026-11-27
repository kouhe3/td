//! td/telegram/net/PublicRsaKeySharedCdn on the Rust side: the dynamic CDN key
//! table driven by a script.
//!
//! ```text
//! public_rsa_cdn_probe <script.txt>
//! ```
//!
//! Script lines: `add <pem_path>`, `get <fp1,fp2,...>`, `drop`, `has`.
//! Output mirrors the C++ probe: `add ok=1` / `add failed message=…`,
//! `get ok=1 fingerprint=…` / `get message=…`, `dropped`, `has=0|1`.
//!
//! The table only needs the fingerprints: get_rsa_key returns the *requested*
//! fingerprint, so the stored RSA value is never observed.

use std::env;
use std::fs;
use std::process::ExitCode;

use num_bigint::BigUint;
use sha1::{Digest, Sha1};

const READ_ERROR: &str = "Error while reading RSA public key";
const NOT_RSA_ERROR: &str = "Key is not an RSA key";
const SIZE_ERROR: &str = "EVP_PKEY_size != 256";
const RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];

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

fn pem_body(pem: &[u8]) -> Option<(Vec<u8>, bool)> {
    let text = std::str::from_utf8(pem).ok()?;
    for (begin, end, pkcs1) in [
        ("-----BEGIN PUBLIC KEY-----", "-----END PUBLIC KEY-----", false),
        ("-----BEGIN RSA PUBLIC KEY-----", "-----END RSA PUBLIC KEY-----", true),
    ] {
        let Some(header) = text.find(begin) else { continue };
        let start = header + begin.len();
        let Some(length) = text[start..].find(end) else { continue };
        return base64_decode(text[start..start + length].as_bytes()).map(|der| (der, pkcs1));
    }
    None
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

fn parse_public_key(der: &[u8], pkcs1: bool) -> Result<(BigUint, BigUint), String> {
    if pkcs1 {
        let mut top = Der::new(der);
        let sequence = top.tlv(0x30).ok_or(READ_ERROR)?;
        if !top.at_end() {
            return Err(READ_ERROR.to_string());
        }
        let mut inner = Der::new(sequence);
        let n = inner.tlv(0x02).ok_or(READ_ERROR)?;
        let e = inner.tlv(0x02).ok_or(READ_ERROR)?;
        return Ok((BigUint::from_bytes_be(n), BigUint::from_bytes_be(e)));
    }
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

fn fingerprint_of_pem(pem: &[u8]) -> Result<i64, String> {
    let (der, pkcs1) = pem_body(pem).ok_or(READ_ERROR)?;
    let (n, e) = parse_public_key(&der, pkcs1)?;
    if (n.bits() + 7) / 8 != 256 {
        return Err(SIZE_ERROR.to_string());
    }
    let mut data = Vec::new();
    data.extend_from_slice(&tl_string(&n.to_bytes_be()));
    data.extend_from_slice(&tl_string(&e.to_bytes_be()));
    let digest = Sha1::digest(&data);
    Ok(i64::from_le_bytes(digest[12..20].try_into().expect("eight bytes")))
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: public_rsa_cdn_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read {}: {error}", args[1]);
            return ExitCode::from(1);
        }
    };

    let mut keys: Vec<i64> = Vec::new();
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut tokens = trimmed.split_whitespace();
        let Some(op) = tokens.next() else { continue };
        match op {
            "add" => {
                let Some(path) = tokens.next() else { continue };
                let pem = fs::read(path).unwrap_or_default();
                match fingerprint_of_pem(&pem) {
                    Ok(fingerprint) => {
                        if !keys.contains(&fingerprint) {
                            keys.push(fingerprint);
                        }
                        out.push_str("add ok=1\n");
                    }
                    Err(message) => out.push_str(&format!("add failed message={message}\n")),
                }
            }
            "get" => {
                let list = tokens.next().unwrap_or("");
                let wanted: Vec<i64> = list.split(',').filter_map(|part| part.parse().ok()).collect();
                match wanted.iter().find(|value| keys.contains(value)) {
                    Some(fingerprint) => out.push_str(&format!("get ok=1 fingerprint={fingerprint}\n")),
                    None => {
                        let items: Vec<String> = wanted.iter().map(|value| value.to_string()).collect();
                        out.push_str(&format!("get message=Unknown CDN fingerprints {{{}}}\n", items.join(", ")));
                    }
                }
            }
            "drop" => {
                keys.clear();
                out.push_str("dropped\n");
            }
            "has" => out.push_str(&format!("has={}\n", keys.is_empty() as u8 ^ 1)),
            other => {
                eprintln!("unknown op {other}");
                return ExitCode::from(2);
            }
        }
    }
    print!("{out}");
    ExitCode::SUCCESS
}