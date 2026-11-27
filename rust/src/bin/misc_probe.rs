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
//! misc_probe strip_empty <hex> <max_length>   strip_empty_characters
//! misc_probe clean_name <hex> <max_length>    clean_name
//! misc_probe clean_input <hex>                clean_input_string
//!
//! `-` as <hex> reads the value from stdin (clean_input's 35000-byte boundary
//! does not fit in a Windows command line).
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

fn read_hex(arg: &str) -> Vec<u8> {
    if arg != "-" {
        return unhex(arg);
    }
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return Vec::new();
    }
    unhex(line.trim())
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

/// The 19 characters strip_empty_characters() folds into a plain space, plus
/// the U+F0000..U+F00FF private-use block that is matched as four bytes.
const SPACE_CHARACTERS: [&[u8; 3]; 19] = [
    b"\xe1\x9a\x80", // U+1680 OGHAM SPACE MARK
    b"\xe1\xa0\x8e", // U+180E MONGOLIAN VOWEL SEPARATOR
    b"\xe2\x80\x80", // U+2000 EN QUAD
    b"\xe2\x80\x81", // U+2001 EM QUAD
    b"\xe2\x80\x82", // U+2002 EN SPACE
    b"\xe2\x80\x83", // U+2003 EM SPACE
    b"\xe2\x80\x84", // U+2004 THREE-PER-EM SPACE
    b"\xe2\x80\x85", // U+2005 FOUR-PER-EM SPACE
    b"\xe2\x80\x86", // U+2006 SIX-PER-EM SPACE
    b"\xe2\x80\x87", // U+2007 FIGURE SPACE
    b"\xe2\x80\x88", // U+2008 PUNCTUATION SPACE
    b"\xe2\x80\x89", // U+2009 THIN SPACE
    b"\xe2\x80\x8a", // U+200A HAIR SPACE
    b"\xe2\x80\xae", // U+202E RIGHT-TO-LEFT OVERRIDE
    b"\xe2\x80\xaf", // U+202F NARROW NO-BREAK SPACE
    b"\xe2\x81\x9f", // U+205F MEDIUM MATHEMATICAL SPACE
    b"\xe2\xa0\x80", // U+2800 BRAILLE PATTERN BLANK
    b"\xe3\x80\x80", // U+3000 IDEOGRAPHIC SPACE
    b"\xef\xbf\xbc", // U+FFFC OBJECT REPLACEMENT CHARACTER
];

fn can_be_first(byte: u8) -> bool {
    matches!(byte, 0xE1 | 0xE2 | 0xE3 | 0xEF | 0xF3)
}

/// utf8_truncate(): `length` counts characters, but the fast path compares it
/// with the *byte* length, and the cut lands on the next character boundary.
fn utf8_truncate(bytes: &[u8], mut length: usize) -> Vec<u8> {
    if bytes.len() > length {
        for index in 0..bytes.len() {
            if (bytes[index] & 0xC0) != 0x80 {
                if length == 0 {
                    return bytes[..index].to_vec();
                }
                length -= 1;
            }
        }
    }
    bytes.to_vec()
}

fn strip_empty_characters(input: &[u8], max_length: usize, strip_rtlo: bool) -> Vec<u8> {
    let mut str = input.to_vec();
    let mut i = 0;
    while i < str.len() && !can_be_first(str[i]) {
        i += 1;
    }
    let mut new_len = i;
    while i < str.len() {
        if can_be_first(str[i]) && i + 3 <= str.len() {
            if str[i] == 0xF3 {
                if i + 4 <= str.len() && str[i + 1] == 0xA0 && (str[i + 2] & 0xFE) == 0x80 {
                    str[new_len] = b' ';
                    new_len += 1;
                    i += 4;
                    continue;
                }
            } else {
                let mut found = false;
                for space_character in SPACE_CHARACTERS {
                    if space_character[0] == str[i]
                        && space_character[1] == str[i + 1]
                        && space_character[2] == str[i + 2]
                    {
                        // U+202E survives unless strip_rtlo is set.
                        if !(str[i] == 0xE2 && str[i + 1] == 0x80 && str[i + 2] == 0xAE) || strip_rtlo {
                            found = true;
                        }
                        break;
                    }
                }
                if found {
                    str[new_len] = b' ';
                    new_len += 1;
                    i += 3;
                    continue;
                }
            }
        }
        str[new_len] = str[i];
        new_len += 1;
        i += 1;
    }
    let truncated = utf8_truncate(trim(&str[..new_len]), max_length);
    let trimmed = trim(&truncated).to_vec();

    // An all-empty string collapses to "".
    let mut i = 0;
    loop {
        if i == trimmed.len() {
            return Vec::new();
        }
        let byte = trimmed[i];
        if byte == b' ' || byte == b'\n' {
            i += 1;
            continue;
        }
        if byte == 0xE2 && trimmed.get(i + 1) == Some(&0x80) {
            let next = trimmed[i + 2];
            if (0x8B..=0x8F).contains(&next) || next == 0xAE {
                i += 3;
                continue;
            }
        }
        if byte == 0xEF && trimmed.get(i + 1) == Some(&0xBB) && trimmed.get(i + 2) == Some(&0xBF) {
            i += 3;
            continue;
        }
        if byte == 0xC2 && trimmed.get(i + 1) == Some(&0xA0) {
            i += 2;
            continue;
        }
        break;
    }
    trimmed
}

fn clean_name(input: &[u8], max_length: usize) -> Vec<u8> {
    let mut str = strip_empty_characters(input, max_length, false);
    let mut new_len = 0;
    let mut is_previous_space = false;
    let mut i = 0;
    while i < str.len() {
        let byte = str[i];
        if byte == b' ' || byte == b'\n' {
            if !is_previous_space {
                str[new_len] = b' ';
                new_len += 1;
                is_previous_space = true;
            }
            i += 1;
            continue;
        }
        if byte == 0xC2 && i + 1 < str.len() && str[i + 1] == 0xA0 {
            if !is_previous_space {
                str[new_len] = b' ';
                new_len += 1;
                is_previous_space = true;
            }
            i += 2;
            continue;
        }
        str[new_len] = byte;
        new_len += 1;
        is_previous_space = false;
        i += 1;
    }
    str.truncate(new_len);
    trim(&str).to_vec()
}

fn clean_input_string(input: &[u8]) -> Option<Vec<u8>> {
    const LENGTH_LIMIT: usize = 35000;
    if std::str::from_utf8(input).is_err() {
        return None;
    }
    let mut str = input.to_vec();
    let str_size = str.len();
    let mut new_size = 0;
    let mut pos = 0;
    while pos < str_size {
        let byte = str[pos];
        let mut step = 1;
        match byte {
            b'\r' => {}
            0..=9 | 11 | 12 | 14..=31 | 32 => {
                str[new_size] = b' ';
                new_size += 1;
            }
            _ => {
                if byte == 0xE2 && pos + 2 < str_size && str[pos + 1] == 0x80 {
                    let next = str[pos + 2];
                    if (0xA8..=0xAE).contains(&next) {
                        step = 3;
                    } else {
                        str[new_size] = byte;
                        new_size += 1;
                    }
                } else if byte == 0xCC && pos + 1 < str_size && matches!(str[pos + 1], 0xB3 | 0xBF | 0x8A) {
                    step = 2;
                } else {
                    str[new_size] = byte;
                    new_size += 1;
                }
            }
        }
        pos += step;
        if new_size >= LENGTH_LIMIT - 3 && new_size > 0 && (str[new_size - 1] & 0xC0) != 0x80 {
            new_size -= 1;
            break;
        }
    }
    str.truncate(new_size);
    Some(replace_offending_characters(&str))
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: misc_probe <username|phone|offending> <hex>");
        return ExitCode::from(2);
    }
    let input = read_hex(&args[2]);
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
        "strip_empty" | "clean_name" => {
            let max_length: usize = args.get(3).and_then(|value| value.parse().ok()).unwrap_or(0);
            let out = if args[1] == "strip_empty" {
                strip_empty_characters(&input, max_length, false)
            } else {
                clean_name(&input, max_length)
            };
            println!("out={}", hex_encode(&out));
            return ExitCode::SUCCESS;
        }
        "clean_input" => {
            match clean_input_string(&input) {
                Some(out) => {
                    println!("ok=1");
                    println!("out={}", hex_encode(&out));
                }
                None => println!("ok=0"),
            }
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