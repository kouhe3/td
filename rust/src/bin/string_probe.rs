//! The string helpers from tdutils misc.h on the Rust side.
//!
//! ```text
//! string_probe <script.txt>
//! ```
//!
//! Script lines (all strings are hex, "-" = empty): `lower`, `upper`, `trim`,
//! `oneline`, `lpad`, `lpad0`, `rpad`, `replace`, `implode`, `split`,
//! `fullsplit`, `pred`.
//!
//! `IS_SPACE` is { ' ', '\t', '\r', '\n', '\0', '\v' } — it excludes '\f'.
//! Case mapping is ASCII-only; bytes >= 0x80 pass through unchanged.

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

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0 | 0x0b)
}

fn is_alpha(byte: u8) -> bool {
    let c = byte | 0x20;
    (b'a'..=b'z').contains(&c)
}

fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

fn is_alnum(byte: u8) -> bool {
    is_alpha(byte) || is_digit(byte)
}

fn is_hex_digit(byte: u8) -> bool {
    if is_digit(byte) {
        return true;
    }
    let c = byte | 0x20;
    (b'a'..=b'f').contains(&c)
}

fn to_lower(data: &[u8]) -> Vec<u8> {
    data.iter().map(|&c| if (b'A'..=b'Z').contains(&c) { c - b'A' + b'a' } else { c }).collect()
}

fn to_upper(data: &[u8]) -> Vec<u8> {
    data.iter().map(|&c| if (b'a'..=b'z').contains(&c) { c - b'a' + b'A' } else { c }).collect()
}

fn trim(data: &[u8]) -> Vec<u8> {
    let mut begin = 0;
    let mut end = data.len();
    while begin < end && is_space(data[begin]) {
        begin += 1;
    }
    while begin < end && is_space(data[end - 1]) {
        end -= 1;
    }
    data[begin..end].to_vec()
}

fn lpad(data: &[u8], size: usize, fill: u8) -> Vec<u8> {
    if data.len() >= size {
        return data.to_vec();
    }
    let mut out = vec![fill; size - data.len()];
    out.extend_from_slice(data);
    out
}

fn rpad(data: &[u8], size: usize, fill: u8) -> Vec<u8> {
    let mut out = data.to_vec();
    if out.len() < size {
        out.extend(std::iter::repeat(fill).take(size - out.len()));
    }
    out
}

fn oneline(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut after_new_line = true;
    for &c in data {
        if c != b'\n' && c != b'\r' {
            if after_new_line {
                if c == b' ' {
                    continue;
                }
                after_new_line = false;
            }
            out.push(c);
        } else if !after_new_line {
            after_new_line = true;
            out.push(b' ');
        }
    }
    while out.last() == Some(&b' ') {
        out.pop();
    }
    out
}

fn replace_with_spaces(data: &[u8], characters: &[u8]) -> Vec<u8> {
    data.iter().map(|&c| if characters.contains(&c) { b' ' } else { c }).collect()
}

fn split_at(data: &[u8], delimiter: u8) -> (Vec<u8>, Vec<u8>) {
    match data.iter().position(|&c| c == delimiter) {
        Some(position) => (data[..position].to_vec(), data[position + 1..].to_vec()),
        None => (data.to_vec(), Vec::new()),
    }
}

fn full_split(data: &[u8], delimiter: u8, max_parts: usize) -> Vec<Vec<u8>> {
    let mut result = Vec::new();
    if data.is_empty() {
        return result;
    }
    let mut rest = data.to_vec();
    while result.len() + 1 < max_parts {
        match rest.iter().position(|&c| c == delimiter) {
            Some(position) => {
                result.push(rest[..position].to_vec());
                rest = rest[position + 1..].to_vec();
            }
            None => break,
        }
    }
    result.push(rest);
    result
}

fn join_parts(parts: &[Vec<u8>]) -> String {
    if parts.is_empty() {
        return "none".to_string();
    }
    parts.iter().map(|part| hex_or_dash(part)).collect::<Vec<_>>().join("|")
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: string_probe <script.txt>");
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
        let number = |index: usize| -> i32 { parts.get(index).map_or(0, |text| text.parse().unwrap_or(0)) };
        match op {
            "lower" | "upper" | "trim" | "oneline" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let result = match op {
                    "lower" => to_lower(&value),
                    "upper" => to_upper(&value),
                    "trim" => trim(&value),
                    _ => oneline(&value),
                };
                println!("{} in={} out={}", op, hex_or_dash(&value), hex_or_dash(&result));
            }
            "lpad" | "lpad0" | "rpad" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let size = number(2) as usize;
                let result = match op {
                    "lpad" => lpad(&value, size, number(3) as u8),
                    "lpad0" => lpad(&value, size, b'0'),
                    _ => rpad(&value, size, number(3) as u8),
                };
                println!("{} in={} size={} out={}", op, hex_or_dash(&value), size, hex_or_dash(&result));
            }
            "replace" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let characters = unhex(parts.get(2).copied().unwrap_or("-"));
                let result = replace_with_spaces(&value, &characters);
                println!(
                    "replace in={} chars={} out={}",
                    hex_or_dash(&value),
                    hex_or_dash(&characters),
                    hex_or_dash(&result)
                );
            }
            "implode" => {
                let delimiter = number(1) as u8;
                let values: Vec<Vec<u8>> = parts[2..].iter().map(|hex| unhex(hex)).collect();
                let mut result = Vec::new();
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        result.push(delimiter);
                    }
                    result.extend_from_slice(value);
                }
                println!(
                    "implode delim={} parts={} out={}",
                    delimiter,
                    join_parts(&values),
                    hex_or_dash(&result)
                );
            }
            "split" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let delimiter = number(2) as u8;
                let (first, second) = split_at(&value, delimiter);
                println!(
                    "split in={} delim={} a={} b={}",
                    hex_or_dash(&value),
                    delimiter,
                    hex_or_dash(&first),
                    hex_or_dash(&second)
                );
            }
            "fullsplit" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let delimiter = number(2) as u8;
                let max_parts = parts.get(3).map_or(0, |text| text.parse::<usize>().unwrap());
                let result = full_split(&value, delimiter, max_parts);
                println!(
                    "fullsplit in={} delim={} max={} parts={}",
                    hex_or_dash(&value),
                    delimiter,
                    max_parts,
                    join_parts(&result)
                );
            }
            "pred" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                for byte in value {
                    println!(
                        "pred b={:02x} space={} alpha={} digit={} alnum={} hex={}",
                        byte,
                        is_space(byte) as i32,
                        is_alpha(byte) as i32,
                        is_digit(byte) as i32,
                        is_alnum(byte) as i32,
                        is_hex_digit(byte) as i32
                    );
                }
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}