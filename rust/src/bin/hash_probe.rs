//! td/utils/Hash.h on the Rust side, for the TD_HAVE_ABSL 0 configuration.
//!
//! ```text
//! hash_probe <script.txt>
//! ```
//!
//! Script lines: `i32` / `i64` / `u32` / `u64` / `b` / `ch` / `uch`,
//! `pair i32|i64 <a> <b>`, `hasher <init> [v…]`, `hasherpair <init> <a> <b>`.
//!
//! `AbslHash` is `TdHash`, i.e. `size_t(v)` — a sign-extending integer
//! conversion, so `int32(-1)` hashes to 18446744073709551615 while
//! `uint32(0xFFFFFFFF)` hashes to 4294967295. Pairs XOR their two halves, so
//! the order does not matter. Hashes are printed as unsigned 64-bit.

use std::env;
use std::fs;
use std::process::ExitCode;

fn hash_i8(value: i8) -> u64 {
    value as i64 as u64
}
fn hash_u8(value: u8) -> u64 {
    value as u64
}
fn hash_i32(value: i32) -> u64 {
    value as i64 as u64
}
fn hash_i64(value: i64) -> u64 {
    value as u64
}
fn hash_u32(value: u32) -> u64 {
    value as u64
}
fn hash_u64(value: u64) -> u64 {
    value
}
fn hash_bool(value: bool) -> u64 {
    value as u64
}
fn hash_pair(first: u64, second: u64) -> u64 {
    0 ^ first ^ second
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: hash_probe <script.txt>");
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
            "i32" | "i64" => {
                let value: i64 = parts[1].parse().unwrap();
                let hash = if op == "i32" { hash_i32(value as i32) } else { hash_i64(value) };
                println!("{} v={} hash={}", op, value, hash);
            }
            "u32" | "u64" => {
                let value: u64 = parts[1].parse().unwrap();
                let hash = if op == "u32" { hash_u32(value as u32) } else { hash_u64(value) };
                println!("{} v={} hash={}", op, value, hash);
            }
            "b" => {
                let value = parts[1] != "0";
                println!("b v={} hash={}", value as i32, hash_bool(value));
            }
            "ch" => {
                let value: u64 = parts[1].parse().unwrap();
                let byte = value as u8;
                println!("ch v={} hash={}", byte as i8 as i32, hash_i8(byte as i8));
            }
            "uch" => {
                let value: u64 = parts[1].parse().unwrap();
                let byte = value as u8;
                println!("uch v={} hash={}", byte, hash_u8(byte));
            }
            "pair" => {
                let kind = parts[1];
                let first: i64 = parts[2].parse().unwrap();
                let second: i64 = parts[3].parse().unwrap();
                let (left, right) = if kind == "i32" {
                    (hash_i32(first as i32), hash_i32(second as i32))
                } else {
                    (hash_i64(first), hash_i64(second))
                };
                println!("pair {} a={} b={} hash={}", kind, first, second, hash_pair(left, right));
            }
            "hasher" => {
                let init: u64 = parts[1].parse().unwrap();
                let values: Vec<u64> = parts[2..].iter().map(|text| text.parse().unwrap()).collect();
                let hash = values.iter().fold(init, |hasher, &value| hasher ^ value);
                println!("hasher init={} n={} hash={}", init, values.len(), hash);
            }
            "hasherpair" => {
                let init: u64 = parts[1].parse().unwrap();
                let first: i64 = parts[2].parse().unwrap();
                let second: i64 = parts[3].parse().unwrap();
                let hash = hash_pair(init, hash_pair(hash_i32(first as i32), hash_i32(second as i32)));
                println!("hasherpair init={} a={} b={} hash={}", init, first, second, hash);
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}