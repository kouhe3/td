//! tdutils base64 / base32 on the Rust side.
//!
//! ```text
//! base64_probe <script.txt>
//! ```
//!
//! Script lines: `enc`, `encurl`, `dec`, `decurl`, `is64`, `is64url`, `is64c`,
//! `is64urlc`, `isany`, `filter`, `b32`, `b32d`.
//!
//! The decoder strips trailing '=' (3+ is an error), requires the padded length
//! to be a multiple of 4, and rejects a short final group whose unused bits are
//! not zero. The url variant uses `-_` and encodes without padding.

use std::env;
use std::fs;
use std::process::ExitCode;

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const B32_LOWER: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
const B32_UPPER: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn table(url: bool) -> [u8; 256] {
    let chars = if url { URL } else { STD };
    let mut values = [64u8; 256];
    for (index, &c) in chars.iter().enumerate() {
        values[c as usize] = index as u8;
    }
    values
}

fn base32_table() -> [u8; 256] {
    let mut values = [32u8; 256];
    for (index, &c) in B32_LOWER.iter().enumerate() {
        values[c as usize] = index as u8;
        values[B32_UPPER[index] as usize] = index as u8;
    }
    values
}

fn encode(input: &[u8], url: bool) -> Vec<u8> {
    let chars = if url { URL } else { STD };
    let mut out = Vec::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < input.len() {
        let left = (input.len() - i).min(3);
        let mut c = (input[i] as u32) << 16;
        i += 1;
        out.push(chars[(c >> 18) as usize]);
        if left != 1 {
            c |= (input[i] as u32) << 8;
            i += 1;
        }
        out.push(chars[((c >> 12) & 63) as usize]);
        if left == 3 {
            c |= input[i] as u32;
            i += 1;
        }
        if left != 1 {
            out.push(chars[((c >> 6) & 63) as usize]);
        } else if !url {
            out.push(b'=');
        }
        if left == 3 {
            out.push(chars[(c & 63) as usize]);
        } else if !url {
            out.push(b'=');
        }
    }
    out
}

fn drop_padding(data: &[u8], url: bool) -> Result<&[u8], &'static str> {
    let mut kept = data;
    let mut padding = 0;
    while let Some((&last, rest)) = kept.split_last() {
        if last != b'=' {
            break;
        }
        kept = rest;
        padding += 1;
    }
    if padding >= 3 {
        return Err("Wrong string padding");
    }
    if (!url || padding > 0) && (kept.len() + padding) % 4 != 0 {
        return Err("Wrong padding length");
    }
    if url && kept.len() % 4 == 1 {
        return Err("Wrong string length");
    }
    Ok(kept)
}

fn decode_impl(data: &[u8], url: bool) -> Result<Vec<u8>, &'static str> {
    let data = drop_padding(data, url)?;
    let values = table(url);
    let mut out = Vec::with_capacity(data.len() / 4 * 3 + ((data.len() & 3) + 1) / 2);
    let mut i = 0;
    while i < data.len() {
        let left = (data.len() - i).min(4);
        let mut c: u32 = 0;
        for position in 0..left {
            let value = values[data[i] as usize];
            i += 1;
            if value == 64 {
                return Err("Wrong character in the string");
            }
            c |= (value as u32) << ((3 - position) * 6);
        }
        out.push(((c >> 16) & 0xff) as u8);
        if left == 2 {
            if c & 0xffff != 0 {
                return Err("Wrong padding in the string");
            }
        } else {
            out.push(((c >> 8) & 0xff) as u8);
            if left == 3 {
                if c & 0xff != 0 {
                    return Err("Wrong padding in the string");
                }
            } else {
                out.push((c & 0xff) as u8);
            }
        }
    }
    Ok(out)
}

fn is_base64_impl(input: &[u8], url: bool) -> bool {
    let data = match drop_padding(input, url) {
        Ok(data) => data,
        Err(_) => return false,
    };
    let values = table(url);
    for &c in data {
        if values[c as usize] == 64 {
            return false;
        }
    }
    if data.len() % 4 == 2 {
        if values[data[data.len() - 1] as usize] & 15 != 0 {
            return false;
        }
    }
    if data.len() % 4 == 3 {
        if values[data[data.len() - 1] as usize] & 3 != 0 {
            return false;
        }
    }
    true
}

fn is_characters_impl(input: &[u8], url: bool) -> bool {
    let values = table(url);
    input.iter().all(|&c| values[c as usize] != 64)
}

fn base64_filter(input: &[u8]) -> Vec<u8> {
    let values = table(false);
    input.iter().filter(|&&c| values[c as usize] != 64 || c == b'=').copied().collect()
}

fn base32_encode(input: &[u8], upper: bool) -> Vec<u8> {
    let chars: &[u8; 32] = if upper { B32_UPPER } else { B32_LOWER };
    let mut out = Vec::with_capacity((input.len() * 8 + 4) / 5);
    let mut c: u32 = 0;
    let mut length: u32 = 0;
    for &byte in input {
        c = (c << 8) | byte as u32;
        length += 8;
        while length >= 5 {
            length -= 5;
            out.push(chars[((c >> length) & 31) as usize]);
        }
    }
    if length != 0 {
        out.push(chars[((c << (5 - length)) & 31) as usize]);
    }
    out
}

fn base32_decode(input: &[u8]) -> Result<Vec<u8>, &'static str> {
    let values = base32_table();
    let mut out = Vec::with_capacity(input.len() * 5 / 8);
    let mut c: u32 = 0;
    let mut length: u32 = 0;
    for &byte in input {
        let value = values[byte as usize];
        if value == 32 {
            return Err("Wrong character in the string");
        }
        c = (c << 5) | value as u32;
        length += 5;
        if length >= 8 {
            length -= 8;
            out.push(((c >> length) & 255) as u8);
        }
    }
    if c & ((1u32 << length) - 1) != 0 {
        return Err("Nonzero padding");
    }
    Ok(out)
}

fn unhex(text: &str) -> Vec<u8> {
    if text == "-" {
        return Vec::new();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let digits = std::str::from_utf8(pair).unwrap();
        out.push(u8::from_str_radix(digits, 16).unwrap());
    }
    out
}

fn hex_or_dash(data: &[u8]) -> String {
    if data.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex_or_dash_text(text: &str) -> String {
    hex_or_dash(text.as_bytes())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: base64_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let op = parts[0];
        match op {
            "enc" | "encurl" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let encoded = encode(&value, op == "encurl");
                println!("{} in={} out={}", op, hex_or_dash(&value), hex_or_dash(&encoded));
            }
            "dec" | "decurl" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                match decode_impl(&value, op == "decurl") {
                    Ok(decoded) => println!("{} in={} ok=1 out={}", op, hex_or_dash(&value), hex_or_dash(&decoded)),
                    Err(message) => {
                        println!("{} in={} ok=0 err={}", op, hex_or_dash(&value), hex_or_dash_text(message))
                    }
                }
            }
            "is64" | "is64url" | "is64c" | "is64urlc" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let result = match op {
                    "is64" => is_base64_impl(&value, false),
                    "is64url" => is_base64_impl(&value, true),
                    "is64c" => is_characters_impl(&value, false),
                    _ => is_characters_impl(&value, true),
                };
                println!("{} in={} r={}", op, hex_or_dash(&value), result as i32);
            }
            "isany" => {
                let code: u8 = parts.get(1).map_or(0, |text| text.parse().unwrap());
                let result = table(false)[code as usize] != 64 || table(true)[code as usize] != 64;
                println!("isany c={} r={}", code, result as i32);
            }
            "filter" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                println!("filter in={} out={}", hex_or_dash(&value), hex_or_dash(&base64_filter(&value)));
            }
            "b32" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let upper: i32 = parts.get(2).map_or(0, |text| text.parse().unwrap());
                println!(
                    "b32 in={} upper={} out={}",
                    hex_or_dash(&value),
                    upper,
                    hex_or_dash(&base32_encode(&value, upper != 0))
                );
            }
            "b32d" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                match base32_decode(&value) {
                    Ok(decoded) => println!("b32d in={} ok=1 out={}", hex_or_dash(&value), hex_or_dash(&decoded)),
                    Err(message) => println!("b32d in={} ok=0 err={}", hex_or_dash(&value), hex_or_dash_text(message)),
                }
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}