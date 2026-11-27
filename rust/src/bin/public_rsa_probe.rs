//! td/telegram/net/PublicRsaKeySharedMain on the Rust side: the two built-in
//! PKCS#1 public keys, their fingerprints, and the list match.
//!
//! ```text
//! public_rsa_probe <main|test> <fp1,fp2,...>
//! ```
//!
//! Output: `ok=1 fingerprint=<matched>` or `message=Unknown Main fingerprints {…}`.
//! The list is printed with braces and ", " separators, empty as `{}`.

use std::env;
use std::process::ExitCode;

use num_bigint::BigUint;
use sha1::{Digest, Sha1};

const MAIN_PEM: &str = "MIIBCgKCAQEA6LszBcC1LGzyr992NzE0ieY+BSaOW622Aa9Bd4ZHLl+TuFQ4lo4g\
5nKaMBwK/BIb9xUfg0Q29/2mgIR6Zr9krM7HjuIcCzFvDtr+L0GQjae9H0pRB2OO\
62cECs5HKhT5DZ98K33vmWiLowc621dQuwKWSQKjWf50XYFw42h21P2KXUGyp2y/\
+aEyZ+uVgLLQbRA1dEjSDZ2iGRy12Mk5gpYc397aYp438fsJoHIgJ2lgMv5h7WY9\
t6N/byY9Nw9p21Og3AoXSL2q/2IJ1WRUhebgAdGVMlV1fkuOQoEzR7EdpqtQD9Cs\
5+bfo3Nhmcyvk5ftB0WkJ9z6bNZ7yxrP8wIDAQAB";

const TEST_PEM: &str = "MIIBCgKCAQEAyMEdY1aR+sCR3ZSJrtztKTKqigvO/vBfqACJLZtS7QMgCGXJ6XIR\
yy7mx66W0/sOFa7/1mAZtEoIokDP3ShoqF4fVNb6XeqgQfaUHd8wJpDWHcR2OFwv\
plUUI1PLTktZ9uW2WE23b+ixNwJjJGwBDJPQEQFBE+vfmH0JP503wr5INS1poWg/\
j25sIWeYPHYeOrFp/eXaqhISP6G+q2IeTaWTXpwZj4LzXq5YOpk4bYEQ6mvRq7D1\
aHWfYmlEGepfaYR8Q0YqvvhYtMte3ITnuSJs171+GDqpdKcSwHnd6FudwGO4pcCO\
j4WcDuXc2CTHgH8gFTNhp/Y8/SpDOhvn9QIDAQAB";

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

fn parse_pkcs1(der: &[u8]) -> Option<(BigUint, BigUint)> {
    let mut top = Der::new(der);
    let sequence = top.tlv(0x30)?;
    if !top.at_end() {
        return None;
    }
    let mut inner = Der::new(sequence);
    let n = inner.tlv(0x02)?;
    let e = inner.tlv(0x02)?;
    Some((BigUint::from_bytes_be(n), BigUint::from_bytes_be(e)))
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
    data.extend_from_slice(&tl_string(&n.to_bytes_be()));
    data.extend_from_slice(&tl_string(&e.to_bytes_be()));
    let digest = Sha1::digest(&data);
    i64::from_le_bytes(digest[12..20].try_into().expect("eight bytes"))
}

fn load(pem_body: &str) -> (BigUint, BigUint) {
    let der = base64_decode(pem_body.as_bytes()).expect("the built-in PEM is valid base64");
    parse_pkcs1(&der).expect("the built-in PEM is a PKCS#1 RSAPublicKey")
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: public_rsa_probe <main|test> <fp1,fp2,...>");
        return ExitCode::from(2);
    }
    let pem = if args[1] == "test" { TEST_PEM } else { MAIN_PEM };
    let (n, e) = load(pem);
    let known = fingerprint(&n, &e);

    let mut fingerprints = Vec::new();
    for part in args[2].split(',') {
        if let Ok(value) = part.parse::<i64>() {
            fingerprints.push(value);
        }
    }

    for wanted in &fingerprints {
        if *wanted == known {
            println!("ok=1 fingerprint={known}");
            return ExitCode::SUCCESS;
        }
    }
    let items: Vec<String> = fingerprints.iter().map(|value| value.to_string()).collect();
    println!("message=Unknown Main fingerprints {{{}}}", items.join(", "));
    ExitCode::SUCCESS
}