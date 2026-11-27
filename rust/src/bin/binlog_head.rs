//! Dump binlog event headers in the same four columns `binlog_dump` prints.
//!
//! Frame layout (tddb/td/db/binlog/BinlogEvent.h:52-54):
//!   [size:u32][id:u64][type:i32][flags:i32][extra:u64][data:size-32][crc32:u32]
//! `binlog_dump` prints the data length (frame size - 32), not the frame size.
//!
//! The first frame is a plaintext service event `type = -3` (AesCtrEncryption);
//! everything after it is AES-256-CTR ciphertext. The key is
//! `PBKDF2-HMAC-SHA256("cucumber", key_salt, 2, 32)` (the default raw DbKey at
//! td/telegram/TdDb.cpp:673), the IV comes from the event, and
//! `HMAC-SHA256(key, "cucumbers everywhere")` must equal the stored key hash.
//!
//! Usage: binlog_head <binlog_file>
//! Output: one line per event, "0x<id:016x> <type> <flags> <data_len>".

use std::env;
use std::fs;
use std::process::ExitCode;

use aes::cipher::{KeyIvInit, StreamCipher};
use hmac::{Hmac, Mac};
use sha2::Sha256;

const HEADER_SIZE: usize = 4 + 8 + 4 + 4 + 8;
const TAIL_SIZE: usize = 4;
const MIN_SIZE: usize = HEADER_SIZE + TAIL_SIZE;
const DEFAULT_KEY: &[u8] = b"cucumber";
const KEY_CHECK: &[u8] = b"cucumbers everywhere";
const AES_CTR_ENCRYPTION: i32 = -3;

type Aes256Ctr = ctr::Ctr128BE<aes::Aes256>;

fn u32_at(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn i32_at(data: &[u8], off: usize) -> i32 {
    u32_at(data, off) as i32
}

/// TL string: 1-byte length (or 0xFE + 3 bytes), data, padded to a 4-byte boundary.
fn tl_string<'a>(buf: &'a [u8], off: &mut usize) -> Result<&'a [u8], String> {
    let first = *buf.get(*off).ok_or("eof at string length")?;
    let (len, hdr) = match first {
        0..=253 => (first as usize, 1),
        254 => {
            if *off + 4 > buf.len() {
                return Err("eof at long string length".to_string());
            }
            let len = buf[*off + 1] as usize
                | (buf[*off + 2] as usize) << 8
                | (buf[*off + 3] as usize) << 16;
            (len, 4)
        }
        _ => return Err(format!("bad string prefix {first}")),
    };
    let start = *off + hdr;
    let end = start + len;
    if end > buf.len() {
        return Err("string data past end".to_string());
    }
    *off = end + (4 - (hdr + len) % 4) % 4;
    if *off > buf.len() {
        return Err("string padding past end".to_string());
    }
    Ok(&buf[start..end])
}

fn emit(id: u64, ty: i32, flags: i32, data_len: usize) {
    println!("0x{id:016x} {ty} {flags} {data_len}");
}

/// Walk frames in `buf`, printing their headers; returns how many were found.
fn frames(buf: &[u8], file_offset: usize) -> Result<usize, String> {
    let mut off = 0usize;
    let mut count = 0usize;
    while off + MIN_SIZE <= buf.len() {
        let size = u32_at(buf, off) as usize;
        if size < MIN_SIZE || off + size > buf.len() {
            return Err(format!(
                "bad frame size={size} at +{off} (file offset {})",
                file_offset + off
            ));
        }
        let id = u64::from_le_bytes(buf[off + 4..off + 12].try_into().unwrap());
        emit(
            id,
            i32_at(buf, off + 12),
            i32_at(buf, off + 16),
            size - MIN_SIZE,
        );
        off += size;
        count += 1;
    }
    if off != buf.len() {
        return Err(format!("trailing {} bytes at +{off}", buf.len() - off));
    }
    Ok(count)
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: binlog_head <binlog_file>");
        return ExitCode::from(2);
    };
    let data = match fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("read {path}: {e}");
            return ExitCode::from(1);
        }
    };
    match run(&data) {
        Ok(events) => {
            eprintln!("events={events}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(data: &[u8]) -> Result<usize, String> {
    if data.len() < MIN_SIZE {
        return Err("file shorter than one frame".to_string());
    }
    let size0 = u32_at(data, 0) as usize;
    if size0 < MIN_SIZE || size0 > data.len() {
        return Err(format!("bad first frame size {size0}"));
    }
    let type0 = i32_at(data, 12);
    emit(
        u64::from_le_bytes(data[4..12].try_into().unwrap()),
        type0,
        i32_at(data, 16),
        size0 - MIN_SIZE,
    );
    let mut events = 1usize;

    if type0 == AES_CTR_ENCRYPTION {
        let payload = &data[HEADER_SIZE..size0 - TAIL_SIZE];
        let mut off = 4; // skip the stored flags word
        let salt = tl_string(payload, &mut off)?;
        let iv = tl_string(payload, &mut off)?;
        let key_hash = tl_string(payload, &mut off)?;

        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(DEFAULT_KEY, salt, 2, &mut key);
        let mut mac = Hmac::<Sha256>::new_from_slice(&key).map_err(|e| e.to_string())?;
        mac.update(KEY_CHECK);
        mac.verify_slice(key_hash)
            .map_err(|_| "key hash mismatch: wrong binlog key".to_string())?;

        let mut rest = data[size0..].to_vec();
        let mut stream = Aes256Ctr::new_from_slices(&key, iv).map_err(|e| e.to_string())?;
        stream.apply_keystream(&mut rest);
        events += frames(&rest, size0)?;
    } else {
        events += frames(&data[size0..], size0)?;
    }
    Ok(events)
}