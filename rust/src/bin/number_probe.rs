//! The integer parsing helpers from tdutils misc.h on the Rust side.
//!
//! ```text
//! number_probe <script.txt>
//! ```
//!
//! Script lines (the input text is hex, "-" = empty): `i32`, `i64`, `u32`,
//! `u64`, `i32s`, `i64s`, `u32s`, `u64s`, `hx32`, `hx64`, `hxs32`, `hxs64`,
//! `hdec`, `henc`.
//!
//! The signed parser accumulates in the unsigned type (wrapping), negates when
//! the value exceeds `T::MAX`, and clamps to `T::MIN` when the negated value is
//! still too large. Parsing stops at the first non-digit and '+' is not a sign.
//! The `*_safe` variants re-print the value and reject anything that does not
//! round-trip.

use std::env;
use std::fs;
use std::process::ExitCode;

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

fn hex_of_string(data: &[u8]) -> String {
    hex_or_dash(data)
}

fn to_integer_signed_bits(bytes: &[u8], bits: u32) -> (u64, bool) {
    // Returns the accumulated unsigned value and whether a sign was seen.
    let mut value: u64 = 0;
    let mut index = 0;
    let mut is_negative = false;
    if !bytes.is_empty() && bytes[0] == b'-' {
        is_negative = true;
        index = 1;
    }
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        value = value.wrapping_mul(10).wrapping_add((bytes[index] - b'0') as u64);
        index += 1;
    }
    (value & mask(bits), is_negative)
}

fn mask(bits: u32) -> u64 {
    if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

fn to_integer_i32(bytes: &[u8]) -> i32 {
    let (mut value, mut is_negative) = to_integer_signed_bits(bytes, 32);
    let max = i32::MAX as u32 as u64;
    if value > max {
        value = (!value).wrapping_add(1) & 0xFFFF_FFFF;
        is_negative = !is_negative;
        if value > max {
            return i32::MIN;
        }
    }
    if is_negative {
        (value as u32 as i32).wrapping_neg()
    } else {
        value as u32 as i32
    }
}

fn to_integer_i64(bytes: &[u8]) -> i64 {
    let (mut value, mut is_negative) = to_integer_signed_bits(bytes, 64);
    let max = i64::MAX as u64;
    if value > max {
        value = (!value).wrapping_add(1);
        is_negative = !is_negative;
        if value > max {
            return i64::MIN;
        }
    }
    if is_negative {
        (value as i64).wrapping_neg()
    } else {
        value as i64
    }
}

fn to_integer_u32(bytes: &[u8]) -> u32 {
    let mut value: u32 = 0;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value.wrapping_mul(10).wrapping_add((byte - b'0') as u32);
    }
    value
}

fn to_integer_u64(bytes: &[u8]) -> u64 {
    let mut value: u64 = 0;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value.wrapping_mul(10).wrapping_add((byte - b'0') as u64);
    }
    value
}

fn hex_to_int(byte: u8) -> u32 {
    if byte.is_ascii_digit() {
        return (byte - b'0') as u32;
    }
    let c = byte | 0x20;
    if (b'a'..=b'f').contains(&c) {
        return (c - b'a') as u32 + 10;
    }
    16
}

fn hex_to_integer_u32(bytes: &[u8]) -> u32 {
    let mut value: u32 = 0;
    for &byte in bytes {
        if hex_to_int(byte) == 16 {
            break;
        }
        value = value.wrapping_mul(16).wrapping_add(hex_to_int(byte));
    }
    value
}

fn hex_to_integer_u64(bytes: &[u8]) -> u64 {
    let mut value: u64 = 0;
    for &byte in bytes {
        if hex_to_int(byte) == 16 {
            break;
        }
        value = value.wrapping_mul(16).wrapping_add(hex_to_int(byte) as u64);
    }
    value
}

fn hex_to_integer_safe_u32(bytes: &[u8]) -> Result<u32, &'static str> {
    if bytes.is_empty() {
        return Err("String is empty");
    }
    let mut value: u32 = 0;
    for &byte in bytes {
        let digit = hex_to_int(byte);
        if digit == 16 {
            return Err("String contains non-hex digit");
        }
        if value > u32::MAX / 16 {
            return Err("String hex number overflows");
        }
        value = value * 16 + digit;
    }
    Ok(value)
}

fn hex_to_integer_safe_u64(bytes: &[u8]) -> Result<u64, &'static str> {
    if bytes.is_empty() {
        return Err("String is empty");
    }
    let mut value: u64 = 0;
    for &byte in bytes {
        let digit = hex_to_int(byte);
        if digit == 16 {
            return Err("String contains non-hex digit");
        }
        if value > u64::MAX / 16 {
            return Err("String hex number overflows");
        }
        value = value * 16 + digit as u64;
    }
    Ok(value)
}

fn parse_error(input: &[u8]) -> String {
    if std::str::from_utf8(input).is_err() {
        return "Strings must be encoded in UTF-8".to_string();
    }
    let mut out = String::from("Can't parse as an integer string \"");
    out.push_str(&String::from_utf8_lossy(input));
    out.push('"');
    out
}

fn safe_roundtrip<T: std::fmt::Display>(bytes: &[u8], value: T) -> Result<String, String> {
    let printed = value.to_string();
    if printed.as_bytes() == bytes {
        Ok(printed)
    } else {
        Err(parse_error(bytes))
    }
}

fn hex_decode(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    if bytes.len() % 2 != 0 {
        return Err("Wrong hex string length");
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = hex_to_int(pair[0]);
        let low = hex_to_int(pair[1]);
        if high == 16 || low == 16 {
            return Err("Wrong hex string");
        }
        out.push((high * 16 + low) as u8);
    }
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: number_probe <script.txt>");
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
        let value = unhex(parts.get(1).copied().unwrap_or("-"));
        let shown = hex_or_dash(&value);
        match op {
            "i32" => println!("i32 in={} out={}", shown, to_integer_i32(&value)),
            "i64" => println!("i64 in={} out={}", shown, to_integer_i64(&value)),
            "u32" => println!("u32 in={} out={}", shown, to_integer_u32(&value)),
            "u64" => println!("u64 in={} out={}", shown, to_integer_u64(&value)),
            "i32s" | "i64s" | "u32s" | "u64s" => {
                let parsed = match op {
                    "i32s" => safe_roundtrip(&value, to_integer_i32(&value)),
                    "i64s" => safe_roundtrip(&value, to_integer_i64(&value)),
                    "u32s" => safe_roundtrip(&value, to_integer_u32(&value)),
                    _ => safe_roundtrip(&value, to_integer_u64(&value)),
                };
                match parsed {
                    Ok(text) => println!("{} in={} ok=1 out={}", op, shown, text),
                    Err(message) => println!("{} in={} ok=0 err={}", op, shown, hex_or_dash(message.as_bytes())),
                }
            }
            "hx32" => println!("hx32 in={} out={}", shown, hex_to_integer_u32(&value)),
            "hx64" => println!("hx64 in={} out={}", shown, hex_to_integer_u64(&value)),
            "hxs32" | "hxs64" => {
                let result = if op == "hxs32" {
                    hex_to_integer_safe_u32(&value).map(|value| value as u64)
                } else {
                    hex_to_integer_safe_u64(&value)
                };
                match result {
                    Ok(number) => println!("{} in={} ok=1 out={}", op, shown, number),
                    Err(message) => println!("{} in={} ok=0 err={}", op, shown, hex_or_dash(message.as_bytes())),
                }
            }
            "hdec" => match hex_decode(&value) {
                Ok(decoded) => println!("hdec in={} ok=1 out={}", shown, hex_of_string(&decoded)),
                Err(message) => println!("hdec in={} ok=0 err={}", shown, hex_or_dash(message.as_bytes())),
            },
            "henc" => {
                let encoded = hex_encode(&value);
                println!("henc in={} out={}", shown, if encoded.is_empty() { "-".to_string() } else { encoded })
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}