//! Simple string sanitizers from td/telegram/misc.cpp on the Rust side.
//!
//! Oracle: tools/fixtures/misc_probe.cpp (same CLI, hex in / hex out).
//!
//! ```text
//! misc_probe username <hex>    clean_username: drop '.', ASCII lower, trim
//! misc_probe phone <hex>       clean_phone_number: keep '0'..'9'
//! misc_probe offending <hex>   replace_offending_characters: a run of
//!                              U+200E/U+200F becomes U+200C... except the last
//! ```
//!
//! trim() removes ' ', '\t', '\r', '\n', '\0' and '\v'.

use std::env;
use std::process::ExitCode;

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

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

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0 | 0x0b)
}

fn trim(bytes: &[u8]) -> &[u8] {
    let mut begin = 0;
    let mut end = bytes.len();
    while begin < end && is_space(bytes[begin]) {
        begin += 1;
    }
    while begin < end && is_space(bytes[end - 1]) {
        end -= 1;
    }
    &bytes[begin..end]
}

fn clean_username(input: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = input
        .iter()
        .filter(|&&byte| byte != b'.')
        .map(|&byte| {
            if byte.is_ascii_uppercase() {
                byte - b'A' + b'a'
            } else {
                byte
            }
        })
        .collect();
    let trimmed = trim(&out).to_vec();
    out = trimmed;
    out
}

fn clean_phone_number(input: &[u8]) -> Vec<u8> {
    input.iter().copied().filter(|byte| byte.is_ascii_digit()).collect()
}

fn replace_offending_characters(input: &[u8]) -> Vec<u8> {
    let mut bytes = input.to_vec();
    let len = bytes.len();
    let mut pos = 0;
    while pos < len {
        if pos + 2 < len && bytes[pos] == 0xe2 && bytes[pos + 1] == 0x80 && (bytes[pos + 2] == 0x8e || bytes[pos + 2] == 0x8f)
        {
            while pos + 5 < len
                && bytes[pos + 3] == 0xe2
                && bytes[pos + 4] == 0x80
                && (bytes[pos + 5] == 0x8e || bytes[pos + 5] == 0x8f)
            {
                bytes[pos + 2] = 0x8c;
                pos += 3;
            }
            pos += 2;
        }
        pos += 1;
    }
    bytes
}

fn is_alpha(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

fn is_valid_username(username: &[u8]) -> bool {
    if username.is_empty() || username.len() > 32 || !is_alpha(username[0]) {
        return false;
    }
    if username
        .iter()
        .any(|&byte| !is_alpha(byte) && !byte.is_ascii_digit() && byte != b'_')
    {
        return false;
    }
    if username.last() == Some(&b'_') {
        return false;
    }
    !username.windows(2).any(|pair| pair[0] == b'_' && pair[1] == b'_')
}

fn is_allowed_username(username: &[u8]) -> bool {
    if !is_valid_username(username) || username.len() < 5 {
        return false;
    }
    let lowered: Vec<u8> = username
        .iter()
        .map(|&byte| {
            if byte.is_ascii_uppercase() {
                byte - b'A' + b'a'
            } else {
                byte
            }
        })
        .collect();
    const RESERVED: [&[u8]; 8] = [
        b"admin",
        b"telegram",
        b"support",
        b"security",
        b"settings",
        b"contacts",
        b"service",
        b"telegraph",
    ];
    !RESERVED.iter().any(|prefix| lowered.starts_with(prefix))
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: misc_probe <username|phone|offending> <hex>");
        return ExitCode::from(2);
    }
    let input = unhex(&args[2]);
    let out = match args[1].as_str() {
        "username" => clean_username(&input),
        "phone" => clean_phone_number(&input),
        "offending" => replace_offending_characters(&input),
        "valid_username" => {
            println!("out={}", is_valid_username(&input) as u8);
            return ExitCode::SUCCESS;
        }
        "allowed_username" => {
            println!("out={}", is_allowed_username(&input) as u8);
            return ExitCode::SUCCESS;
        }
        other => {
            eprintln!("unknown mode {other}");
            return ExitCode::from(2);
        }
    };
    println!("out={}", hex_encode(&out));
    ExitCode::SUCCESS
}