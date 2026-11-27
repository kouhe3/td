//! The structural part of tdutils utf8.h on the Rust side (no unicode tables).
//!
//! ```text
//! utf8_probe <script.txt>
//! ```
//!
//! Script lines: `check`, `len`, `first`, `next`, `app`, `trunc`, `substr`,
//! `substr2`, `u16len`, `u16trunc`, `u16substr`, `u16substr2`.
//!
//! `check_utf8` is reimplemented as the same shift-based DFA: each byte picks a
//! 64-bit word of 6-bit transitions and the next state is that word shifted by
//! the current state. `utf8_truncate` compares the byte size with the character
//! budget, and `utf8_utf16_length` vs `utf8_utf16_truncate` disagree about
//! F8..FF.

use std::env;
use std::fs;
use std::process::ExitCode;

const ACCEPT: u32 = 6;
const NEED_1: u32 = 12;
const NEED_2: u32 = 18;
const NEED_3: u32 = 24;
const AFTER_E0: u32 = 30;
const AFTER_ED: u32 = 36;
const AFTER_F0: u32 = 42;
const AFTER_F4: u32 = 48;

fn transition(from: u32, to: u32) -> u64 {
    (to as u64) << from
}

fn utf8_transitions(c: u32) -> u64 {
    if c < 0x80 {
        return transition(ACCEPT, ACCEPT);
    }
    if c < 0xc0 {
        let mut result =
            transition(NEED_1, ACCEPT) | transition(NEED_2, NEED_1) | transition(NEED_3, NEED_2);
        if c < 0xa0 {
            result |= transition(AFTER_ED, NEED_1);
        } else {
            result |= transition(AFTER_E0, NEED_1);
        }
        if c < 0x90 {
            result |= transition(AFTER_F4, NEED_2);
        } else {
            result |= transition(AFTER_F0, NEED_2);
        }
        return result;
    }
    if c < 0xc2 {
        return 0;
    }
    if c < 0xe0 {
        return transition(ACCEPT, NEED_1);
    }
    if c == 0xe0 {
        return transition(ACCEPT, AFTER_E0);
    }
    if c == 0xed {
        return transition(ACCEPT, AFTER_ED);
    }
    if c < 0xf0 {
        return transition(ACCEPT, NEED_2);
    }
    if c == 0xf0 {
        return transition(ACCEPT, AFTER_F0);
    }
    if c < 0xf4 {
        return transition(ACCEPT, NEED_3);
    }
    if c == 0xf4 {
        return transition(ACCEPT, AFTER_F4);
    }
    0
}

fn check_utf8(data: &[u8]) -> bool {
    let mut table = [0u64; 256];
    for (c, entry) in table.iter_mut().enumerate() {
        *entry = utf8_transitions(c as u32);
    }
    let mut state = ACCEPT as u64;
    for &byte in data {
        state = table[byte as usize] >> (state & 63);
    }
    state & 63 == ACCEPT as u64
}

fn utf8_length(data: &[u8]) -> usize {
    data.iter().filter(|&&c| c & 0xc0 != 0x80).count()
}

fn next_utf8(data: &[u8]) -> (u32, usize) {
    let a = data[0] as u32;
    if a & 0x80 == 0 {
        (a, 1)
    } else if a & 0x20 == 0 {
        (((a & 0x1f) << 6) | (data[1] as u32 & 0x3f), 2)
    } else if a & 0x10 == 0 {
        (((a & 0x0f) << 12) | ((data[1] as u32 & 0x3f) << 6) | (data[2] as u32 & 0x3f), 3)
    } else {
        (
            ((a & 0x07) << 18)
                | ((data[1] as u32 & 0x3f) << 12)
                | ((data[2] as u32 & 0x3f) << 6)
                | (data[3] as u32 & 0x3f),
            4,
        )
    }
}

fn append_utf8(code: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(4);
    if code <= 0x7f {
        out.push(code as u8);
    } else if code <= 0x7ff {
        out.push((0xc0 | (code >> 6)) as u8);
        out.push((0x80 | (code & 0x3f)) as u8);
    } else if code <= 0xffff {
        out.push((0xe0 | (code >> 12)) as u8);
        out.push((0x80 | ((code >> 6) & 0x3f)) as u8);
        out.push((0x80 | (code & 0x3f)) as u8);
    } else {
        out.push((0xf0 | (code >> 18)) as u8);
        out.push((0x80 | ((code >> 12) & 0x3f)) as u8);
        out.push((0x80 | ((code >> 6) & 0x3f)) as u8);
        out.push((0x80 | (code & 0x3f)) as u8);
    }
    out
}

fn utf8_truncate(data: &[u8], mut length: usize) -> &[u8] {
    if data.len() > length {
        for index in 0..data.len() {
            if data[index] & 0xc0 != 0x80 {
                if length == 0 {
                    return &data[..index];
                }
                length -= 1;
            }
        }
    }
    data
}

fn utf8_substr(data: &[u8], offset: usize) -> &[u8] {
    if offset == 0 {
        return data;
    }
    let position = utf8_truncate(data, offset).len();
    &data[position..]
}

fn utf8_utf16_length(data: &[u8]) -> usize {
    let mut result = 0;
    for &c in data {
        result += (c & 0xc0 != 0x80) as usize + ((c & 0xf8) == 0xf0) as usize;
    }
    result
}

fn utf8_utf16_truncate(data: &[u8], mut length: usize) -> &[u8] {
    for index in 0..data.len() {
        let c = data[index];
        if c & 0xc0 != 0x80 {
            if length == 0 {
                return &data[..index];
            }
            length = length.wrapping_sub(1);
            if c >= 0xf0 {
                length = length.wrapping_sub(1);
            }
        }
    }
    data
}

fn utf8_utf16_substr(data: &[u8], offset: usize) -> &[u8] {
    if offset == 0 {
        return data;
    }
    let position = utf8_utf16_truncate(data, offset).len();
    &data[position..]
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

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: utf8_probe <script.txt>");
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
        let number = |index: usize| -> usize { parts.get(index).map_or(0, |text| text.parse().unwrap_or(0)) };
        match op {
            "check" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                println!("check in={} r={}", hex_or_dash(&value), check_utf8(&value) as i32);
            }
            "len" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                println!("len in={} n={}", hex_or_dash(&value), utf8_length(&value));
            }
            "first" => {
                let code = number(1) as u8;
                println!("first c={} r={}", code, (code & 0xc0 != 0x80) as i32);
            }
            "next" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let (code, used) = next_utf8(&value);
                println!("next in={} code={} used={}", hex_or_dash(&value), code, used);
            }
            "app" => {
                let code = number(1) as u32;
                println!("app code={} out={}", code, hex_or_dash(&append_utf8(code)));
            }
            "trunc" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let length = number(2);
                println!(
                    "trunc in={} n={} out={}",
                    hex_or_dash(&value),
                    length,
                    hex_or_dash(utf8_truncate(&value, length))
                );
            }
            "substr" | "substr2" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let offset = number(2);
                let result = if op == "substr" {
                    utf8_substr(&value, offset).to_vec()
                } else {
                    let sliced = utf8_substr(&value, offset);
                    utf8_truncate(sliced, number(3)).to_vec()
                };
                println!("{} in={} off={} out={}", op, hex_or_dash(&value), offset, hex_or_dash(&result));
            }
            "u16len" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                println!("u16len in={} n={}", hex_or_dash(&value), utf8_utf16_length(&value));
            }
            "u16trunc" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let length = number(2);
                println!(
                    "u16trunc in={} n={} out={}",
                    hex_or_dash(&value),
                    length,
                    hex_or_dash(utf8_utf16_truncate(&value, length))
                );
            }
            "u16substr" | "u16substr2" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let offset = number(2);
                let result = if op == "u16substr" {
                    utf8_utf16_substr(&value, offset).to_vec()
                } else {
                    let sliced = utf8_utf16_substr(&value, offset);
                    utf8_utf16_truncate(sliced, number(3)).to_vec()
                };
                println!("{} in={} off={} out={}", op, hex_or_dash(&value), offset, hex_or_dash(&result));
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}