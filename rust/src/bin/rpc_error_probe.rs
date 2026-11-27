//! Encode rpc_error#2144ca19 fields the way TDLib's generated storer does.
//!
//! TDLib's `rpc_error::store` writes only the fields (no constructor id); the
//! id is written by the boxed wrapper. This probe prints both the field bytes
//! and the boxed object, so the checker can compare fields with the oracle and
//! pin the id independently.
//!
//! Usage: rpc_error_probe <fixture.json>
//! Output: fields=<hex>, full=<hex>

use std::env;
use std::fs;
use std::process::ExitCode;

const RPC_ERROR: u32 = 0x2144_ca19;

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

fn push_tl_string(out: &mut Vec<u8>, s: &[u8]) {
    assert!(s.len() < 254, "long strings are not needed here");
    out.push(s.len() as u8);
    out.extend_from_slice(s);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: rpc_error_probe <fixture.json>");
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

    let code: i32 = get("code").parse().unwrap();
    let message = get("message").as_bytes().to_vec();

    let mut fields = Vec::new();
    fields.extend_from_slice(&code.to_le_bytes());
    push_tl_string(&mut fields, &message);

    let mut full = Vec::with_capacity(4 + fields.len());
    full.extend_from_slice(&RPC_ERROR.to_le_bytes());
    full.extend_from_slice(&fields);

    println!("fields={}", hex_encode(&fields));
    println!("full={}", hex_encode(&full));
    ExitCode::SUCCESS
}