//! Frame a payload the way the MTProto intermediate transport does.
//!
//! Intermediate: 4-byte little-endian length (payload + padding, excluding the
//! prefix), with bit 31 set for quick-ack packets; the stream starts with the
//! magic 0xeeeeeeee (or 0xdddddddd for padded intermediate).
//!
//! Oracle: tools/fixtures/framing_probe.cpp calls TDLib's
//! tcp::IntermediateTransport::write_prepare_inplace / init_output_stream.
//!
//! Usage: framing_probe <fixture.json>
//! Output: magic=, length=, packet=

use std::env;
use std::fs;
use std::process::ExitCode;

fn hex_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let val = |c: u8| -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("bad hex digit"),
        }
    };
    let mut i = 0;
    while i + 1 < bytes.len() {
        out.push((val(bytes[i]) << 4) | val(bytes[i + 1]));
        i += 2;
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

fn json_field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\"");
    let start = text.find(&key)? + key.len();
    let rest = &text[start..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(&stripped[..end])
    } else {
        let end = rest
            .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
            .unwrap_or(rest.len());
        Some(rest[..end].trim())
    }
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: framing_probe <fixture.json>");
        return ExitCode::from(2);
    };
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("read {path}: {e}");
            return ExitCode::from(1);
        }
    };
    let get = |name: &str| json_field(&text, name).unwrap_or_else(|| panic!("missing {name}"));

    let payload = hex_decode(get("frame_payload"));
    let with_padding = get("with_padding") != "0";
    let quick_ack = get("quick_ack") != "0";

    let magic: u32 = if with_padding { 0xdddd_dddd } else { 0xeeee_eeee };
    let mut length = payload.len() as u32;
    if quick_ack {
        length |= 1 << 31;
    }

    let mut packet = Vec::with_capacity(4 + payload.len());
    packet.extend_from_slice(&length.to_le_bytes());
    packet.extend_from_slice(&payload);

    println!("magic={:08x}", magic.to_le());
    println!("length={}", length & 0x7fff_ffff);
    println!("packet={}", hex_encode(&packet));
    ExitCode::SUCCESS
}