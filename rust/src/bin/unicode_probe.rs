//! tdutils' unicode tables on the Rust side, built from the generated `unicode_tables.rs`.
//!
//! ```text
//! unicode_probe <script.txt>
//! ```
//!
//! Script lines: `cat`, `prep`, `low`, `dia`, `sweep`, `utfl`, `words`.
//!
//! The four table lookups mirror `unicode.cpp`: under `TABLE_SIZE` the int16 tables answer
//! directly, above it `binary_search_ranges` walks the (range_begin, replacement) pairs.
//! A negative replacement maps the rest of its range linearly (`code - begin + ~value`),
//! a replacement above 0x10ffff selects one of the bit toggles at 0x200000..0x200002, and
//! a code above 0x10ffff returns 0 outright — except for `get_unicode_simple_category`,
//! which has no such guard and is therefore only fed codes inside its jump table.
//! `utf8_to_lower` / `utf8_get_search_words` reproduce `utf8.cpp` on top of these lookups,
//! including the byte-wise `next_utf8_unsafe` (missing bytes read as 0, like the string
//! terminator the C++ version relies on).

use std::env;
use std::fs;
use std::io::{BufWriter, Write};
use std::process::ExitCode;

#[path = "../unicode_tables.rs"]
mod unicode_tables;

fn unicode_simple_category(code: u32) -> u32 {
    let packed = (code << 5).wrapping_add(30);
    let ranges = &unicode_tables::UNICODE_SIMPLE_CATEGORY_RANGES;
    let index = ranges.partition_point(|&boundary| boundary <= packed);
    (ranges[index - 1] & 31) as u32
}

fn binary_search_ranges(ranges: &[i32], code: u32) -> u32 {
    if code > 0x10ffff {
        return 0;
    }
    let code_int = code as i32;
    let mut left = 0usize;
    let mut right = ranges.len();
    while left < right {
        let middle = ((left + right + 2) >> 2) << 1;
        let begin = match ranges.get(middle) {
            Some(begin) => *begin,
            None => i32::MAX,
        };
        if begin <= code_int {
            left = middle;
        } else {
            right = middle - 2;
        }
    }
    let (start, value) = match (ranges.get(left), ranges.get(left + 1)) {
        (Some(start), Some(value)) => (*start, *value),
        _ => return 0,
    };
    if value < 0 {
        return (code_int - start + !value) as u32;
    }
    if value <= 0x10ffff {
        return value as u32;
    }
    match value - 0x200000 {
        0 => code & 0xffff_fffe,
        1 => code | 1,
        2 => (code - 1) | 1,
        _ => 0,
    }
}

fn prepare_search_character(code: u32) -> u32 {
    if code < unicode_tables::TABLE_SIZE {
        unicode_tables::PREPARE_SEARCH_CHARACTER_TABLE[code as usize] as u32
    } else {
        binary_search_ranges(&unicode_tables::PREPARE_SEARCH_CHARACTER_RANGES, code)
    }
}

fn unicode_to_lower(code: u32) -> u32 {
    if code < unicode_tables::TABLE_SIZE {
        unicode_tables::TO_LOWER_TABLE[code as usize] as u32
    } else {
        binary_search_ranges(&unicode_tables::TO_LOWER_RANGES, code)
    }
}

fn remove_diacritics(code: u32) -> u32 {
    if code < unicode_tables::TABLE_SIZE {
        unicode_tables::WITHOUT_DIACRITICS_TABLE[code as usize] as u32
    } else {
        binary_search_ranges(&unicode_tables::WITHOUT_DIACRITICS_RANGES, code)
    }
}

fn next_utf8(data: &[u8], pos: usize) -> (u32, usize) {
    let at = |offset: usize| data.get(pos + offset).copied().unwrap_or(0) as u32;
    let a = at(0);
    if a & 0x80 == 0 {
        (a, pos + 1)
    } else if a & 0x20 == 0 {
        (((a & 0x1f) << 6) | (at(1) & 0x3f), pos + 2)
    } else if a & 0x10 == 0 {
        (((a & 0x0f) << 12) | ((at(1) & 0x3f) << 6) | (at(2) & 0x3f), pos + 3)
    } else {
        (
            ((a & 0x07) << 18) | ((at(1) & 0x3f) << 12) | ((at(2) & 0x3f) << 6) | (at(3) & 0x3f),
            pos + 4,
        )
    }
}

fn append_utf8(out: &mut Vec<u8>, code: u32) {
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
}

fn utf8_to_lower(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut pos = 0usize;
    while pos < data.len() {
        let (code, next) = next_utf8(data, pos);
        pos = next;
        append_utf8(&mut out, unicode_to_lower(code));
    }
    out
}

fn get_search_words(data: &[u8]) -> Vec<Vec<u8>> {
    let mut words = Vec::new();
    let mut word = Vec::new();
    let mut in_word = false;
    let mut pos = 0usize;
    while pos < data.len() {
        let (code, next) = next_utf8(data, pos);
        pos = next;
        let code = prepare_search_character(code);
        if code == 0 {
            continue;
        }
        if code == ' ' as u32 {
            if in_word {
                words.push(std::mem::take(&mut word));
                in_word = false;
            }
        } else {
            in_word = true;
            append_utf8(&mut word, remove_diacritics(code));
        }
    }
    if in_word {
        words.push(word);
    }
    words
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
        eprintln!("usage: unicode_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let code_at = |index: usize| -> u32 {
            parts
                .get(index)
                .map_or(0, |text| text.parse().unwrap_or(0))
        };
        let op = parts[0];
        let result = match op {
            "cat" | "prep" | "low" | "dia" => {
                let code = code_at(1);
                let value = match op {
                    "cat" => unicode_simple_category(code),
                    "prep" => prepare_search_character(code),
                    "low" => unicode_to_lower(code),
                    _ => remove_diacritics(code),
                };
                writeln!(out, "{op} c=0x{code:x} v={value}")
            }
            "sweep" => {
                let lo = code_at(1) as u64;
                let hi = code_at(2) as u64;
                let mut result = Ok(());
                let mut code = lo;
                while code <= hi {
                    let c = code as u32;
                    result = writeln!(
                        out,
                        "c=0x{c:x} cat={} prep={} low={} dia={}",
                        unicode_simple_category(c),
                        prepare_search_character(c),
                        unicode_to_lower(c),
                        remove_diacritics(c)
                    );
                    if result.is_err() {
                        break;
                    }
                    code += 1;
                }
                result
            }
            "utfl" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                writeln!(
                    out,
                    "utfl in={} out={}",
                    hex_or_dash(&value),
                    hex_or_dash(&utf8_to_lower(&value))
                )
            }
            "words" => {
                let value = unhex(parts.get(1).copied().unwrap_or("-"));
                let words = get_search_words(&value);
                let mut result = writeln!(out, "words in={} n={}", hex_or_dash(&value), words.len());
                if words.is_empty() {
                    result = result.and_then(|_| writeln!(out, "-"));
                }
                for word in &words {
                    if result.is_err() {
                        break;
                    }
                    result = writeln!(out, "{}", hex_or_dash(word));
                }
                result
            }
            other => writeln!(out, "unknown {other}"),
        };
        if result.is_err() {
            return ExitCode::from(1);
        }
    }
    if out.flush().is_err() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}