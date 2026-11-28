//! td/utils/crypto.h on the Rust side: `pbkdf2_sha256`, `hmac_sha256`,
//! `crc32` and the stateful `AesCtrState` used by `AesCtrEncryptionEvent`.
//!
//! ```text
//! crypto_probe <script.txt>
//! ```
//!
//! Script lines, every payload hex-encoded with `-` for the empty string:
//! `pbkdf2 <password> <salt> <iterations> <outsize>`, `hmac <key> <message>`,
//! `crc32 <data>`, `ctr <key> <iv> <data>`, and
//! `ctrsplit <key> <iv> <part> [part…]`, which inits one state and transforms
//! the parts through separate calls, proving that the keystream resumes.
//!
//! Output lines: `pbkdf2 it=<n> out=<hex>`, `hmac out=<hex>`,
//! `crc32 v=<8 hex>`, `ctr n=<bytes> out=<hex>`,
//! `ctrsplit n=<bytes> parts=<count> out=<hex>`.
//!
//! The C++ preconditions (`CHECK(dest.size() == 32)`, `CHECK(iteration_count
//! > 0)`, 32-byte key, 16-byte iv) abort rather than produce bytes, so the same
//! inputs abort here; no output line is written and the exit status is nonzero,
//! which is the only criterion those inputs have.

use std::env;
use std::fs;
use std::process::ExitCode;

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes256;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// RFC 1952 CRC-32: reflected polynomial 0xEDB88320, init and final xor
/// 0xFFFFFFFF. `zlib`'s `crc32(0, …)` is the same function.
const fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

static CRC32_TABLE: [u32; 256] = crc32_table();

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = (crc >> 8) ^ CRC32_TABLE[((crc ^ byte as u32) & 0xFF) as usize];
    }
    crc ^ 0xFFFF_FFFF
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut dest = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, iterations, &mut dest);
    dest
}

/// The `AesCtrState`/`Evp::init_encrypt_ctr` state machine: a 128-bit
/// big-endian counter is encrypted to form each keystream block and then
/// incremented with full carry, so a 16-byte-aligned continuation is just the
/// next block of the same stream (no reset, no extra block).
struct AesCtrState {
    cipher: Aes256,
    counter: [u8; 16],
    keystream: [u8; 16],
    pos: usize,
}

impl AesCtrState {
    fn new(key: &[u8], iv: &[u8]) -> Self {
        let cipher = Aes256::new(GenericArray::from_slice(key));
        let mut counter = [0u8; 16];
        counter.copy_from_slice(iv);
        AesCtrState { cipher, counter, keystream: [0u8; 16], pos: 16 }
    }

    fn transform(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        for &byte in data {
            if self.pos == 16 {
                let mut block = GenericArray::from(self.counter);
                self.cipher.encrypt_block(&mut block);
                self.keystream = block.into();
                for index in (0..16).rev() {
                    self.counter[index] = self.counter[index].wrapping_add(1);
                    if self.counter[index] != 0 {
                        break;
                    }
                }
                self.pos = 0;
            }
            out.push(byte ^ self.keystream[self.pos]);
            self.pos += 1;
        }
        out
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_hex(token: &str) -> Option<Vec<u8>> {
    if token == "-" {
        return Some(Vec::new());
    }
    let bytes = token.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        out.push((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?);
    }
    Some(out)
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The td preconditions are CHECKs, i.e. the process dies before any bytes
/// exist; mirror that instead of inventing a value.
fn check_failed(message: &str) -> ExitCode {
    eprintln!("Check `{message}` failed");
    std::process::abort();
}

fn run_script(path: &str) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {path}: {error}");
            return ExitCode::from(2);
        }
    };
    for (index, line) in text.lines().enumerate() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() || tokens[0].starts_with('#') {
            continue;
        }
        let line_number = index + 1;
        match tokens[0] {
            "pbkdf2" => {
                if tokens.len() != 5 {
                    eprintln!("line {line_number}: bad pbkdf2 arguments");
                    return ExitCode::from(2);
                }
                let (Some(password), Some(salt)) = (parse_hex(tokens[1]), parse_hex(tokens[2])) else {
                    eprintln!("line {line_number}: bad pbkdf2 hex");
                    return ExitCode::from(2);
                };
                let Ok(iterations) = tokens[3].parse::<i32>() else {
                    eprintln!("line {line_number}: bad pbkdf2 iterations");
                    return ExitCode::from(2);
                };
                let Ok(outsize) = tokens[4].parse::<usize>() else {
                    eprintln!("line {line_number}: bad pbkdf2 outsize");
                    return ExitCode::from(2);
                };
                if outsize != 32 {
                    return check_failed("dest.size() == static_cast<size_t>(hash_size)");
                }
                if iterations <= 0 {
                    return check_failed("iteration_count > 0");
                }
                let dest = pbkdf2_sha256(&password, &salt, iterations as u32);
                println!("pbkdf2 it={iterations} out={}", to_hex(&dest));
            }
            "hmac" => {
                if tokens.len() != 3 {
                    eprintln!("line {line_number}: bad hmac arguments");
                    return ExitCode::from(2);
                }
                let (Some(key), Some(message)) = (parse_hex(tokens[1]), parse_hex(tokens[2])) else {
                    eprintln!("line {line_number}: bad hmac hex");
                    return ExitCode::from(2);
                };
                let dest = hmac_sha256(&key, &message);
                println!("hmac out={}", to_hex(&dest));
            }
            "crc32" => {
                if tokens.len() != 2 {
                    eprintln!("line {line_number}: bad crc32 arguments");
                    return ExitCode::from(2);
                }
                let Some(data) = parse_hex(tokens[1]) else {
                    eprintln!("line {line_number}: bad crc32 hex");
                    return ExitCode::from(2);
                };
                println!("crc32 v={:08x}", crc32(&data));
            }
            "ctr" | "ctrsplit" => {
                if tokens.len() < 3 {
                    eprintln!("line {line_number}: bad {} arguments", tokens[0]);
                    return ExitCode::from(2);
                }
                let (Some(key), Some(iv)) = (parse_hex(tokens[1]), parse_hex(tokens[2])) else {
                    eprintln!("line {line_number}: bad {} hex", tokens[0]);
                    return ExitCode::from(2);
                };
                if key.len() != 32 {
                    return check_failed("key.size() == 32");
                }
                if iv.len() != 16 {
                    return check_failed("iv.size() == 16");
                }
                let mut state = AesCtrState::new(&key, &iv);
                if tokens[0] == "ctr" {
                    if tokens.len() != 4 {
                        eprintln!("line {line_number}: bad ctr arguments");
                        return ExitCode::from(2);
                    }
                    let Some(data) = parse_hex(tokens[3]) else {
                        eprintln!("line {line_number}: bad ctr hex");
                        return ExitCode::from(2);
                    };
                    let ciphertext = state.transform(&data);
                    println!("ctr n={} out={}", data.len(), to_hex(&ciphertext));
                } else {
                    let parts = &tokens[3..];
                    if parts.is_empty() {
                        eprintln!("line {line_number}: ctrsplit needs at least one part");
                        return ExitCode::from(2);
                    }
                    let mut ciphertext = Vec::new();
                    for (part_index, part_token) in parts.iter().enumerate() {
                        let Some(part) = parse_hex(part_token) else {
                            eprintln!("line {line_number}: bad ctrsplit part {}", part_index + 1);
                            return ExitCode::from(2);
                        };
                        ciphertext.extend_from_slice(&state.transform(&part));
                    }
                    println!(
                        "ctrsplit n={} parts={} out={}",
                        ciphertext.len(),
                        parts.len(),
                        to_hex(&ciphertext)
                    );
                }
            }
            other => {
                eprintln!("line {line_number}: unknown command {other}");
                return ExitCode::from(2);
            }
        }
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: crypto_probe <script.txt>");
        return ExitCode::from(2);
    }
    run_script(&args[1])
}