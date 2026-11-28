//! Judge `tdutils/td/utils/emoji.h` against the C++ oracle
//! (`tools/fixtures/emoji_probe.cpp`).
//!
//!   emoji_probe <script.txt>
//!
//! Commands (`-` is the empty string, every payload is hex):
//!   is <hex> | find <hex> | fp <hex> | rfp <hex> | rem <hex> <0|1> | rs <hex>

#[allow(dead_code)]
#[path = "../emoji_table.rs"]
mod emoji_table;
#[allow(dead_code)]
#[path = "../emoji.rs"]
mod emoji;

use std::io::BufRead;

use emoji::{
    find_emojis, get_fitzpatrick_modifier, is_emoji, remove_emoji_modifiers, remove_emoji_selectors,
    remove_fitzpatrick_modifier,
};

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text == "-" {
        return Some(Vec::new());
    }
    if text.len() % 2 != 0 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for index in (0..bytes.len()).step_by(2) {
        let nibble = |c: u8| -> Option<u8> {
            match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                _ => None,
            }
        };
        out.push((nibble(bytes[index])? << 4) | nibble(bytes[index + 1])?);
    }
    Some(out)
}

fn to_hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn run_script(script_path: &str) -> i32 {
    let file = match std::fs::File::open(script_path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("cannot read {script_path}: {error}");
            return 2;
        }
    };
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() || tokens[0].starts_with('#') {
            continue;
        }
        let command = tokens[0];
        if command == "rem" {
            let (Some(hex), Some(flag)) = (tokens.get(1).copied(), tokens.get(2).copied()) else {
                println!("reject rem argc={}", tokens.len());
                continue;
            };
            if tokens.len() != 3 || (flag != "0" && flag != "1") {
                println!("reject rem argc={}", tokens.len());
                continue;
            }
            let Some(bytes) = from_hex(hex) else {
                println!("reject rem argc={}", tokens.len());
                continue;
            };
            let out = remove_emoji_modifiers(&bytes, flag == "1");
            println!("rem {hex} {flag} {}", to_hex(&out));
            continue;
        }
        let Some(hex) = tokens.get(1).copied() else {
            println!("reject {command} argc={}", tokens.len());
            continue;
        };
        if tokens.len() != 2 {
            println!("reject {command} argc={}", tokens.len());
            continue;
        }
        let Some(bytes) = from_hex(hex) else {
            println!("reject {command} argc={}", tokens.len());
            continue;
        };
        match command {
            "is" => println!("is {hex} {}", i32::from(is_emoji(&bytes))),
            "find" => {
                let parts: Vec<String> = find_emojis(&bytes)
                    .into_iter()
                    .map(|(start, end)| to_hex(&bytes[start..end]))
                    .collect();
                println!("find {hex} [{}]", parts.join(";"));
            }
            "fp" => println!("fp {hex} {}", get_fitzpatrick_modifier(&bytes)),
            "rfp" => println!("rfp {hex} {}", to_hex(remove_fitzpatrick_modifier(&bytes))),
            "rs" => println!("rs {hex} {}", to_hex(&remove_emoji_selectors(&bytes))),
            other => println!("reject unknown={other}"),
        }
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: emoji_probe <script.txt>");
        std::process::exit(2);
    }
    std::process::exit(run_script(&args[1]));
}