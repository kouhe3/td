//! The binlog frame format shared by the `binlog_*` probes.
//!
//! Frame layout (tddb/td/db/binlog/BinlogEvent.h:52-54), all little endian:
//!   [size:u32][id:u64][type:i32][flags:i32][extra:u64][data:size-32][crc32:u32]
//! `size` counts the whole frame including the trailing crc, `extra` is always 0
//! (BinlogEvent::create_raw stores a constant 0 there), and the crc is td::crc32
//! -- zlib's -- over the first `size - 4` bytes (BinlogEvent.cpp:66-70).
//!
//! Only the parts that were the same code are here (`binlog_head`'s `u32_at`
//! differed just in its parameter names). Kept per-bin because they differ:
//! `to_hex` (the write lane returns `'-'`, the KV lane `"-".to_owned()`), the
//! frame struct and `make_event` (`LogEvent` vs `Frame`), `walk_frames` (the
//! write lane also returns the consumed length), `binlog_head`'s
//! `Result`-returning `tl_string` and its decrypting walker.

pub const HEADER_SIZE: usize = 4 + 8 + 4 + 4 + 8;
pub const TAIL_SIZE: usize = 4;
pub const MIN_SIZE: usize = HEADER_SIZE + TAIL_SIZE;
pub const FLAG_REWRITE: i32 = 1;
pub const SERVICE_EMPTY: i32 = -2;
pub const LAZY_FLUSH_THRESHOLD: usize = 1 << 14;
/// `Binlog::add_event`: (min file size, dead-ratio divisor)
pub const REINDEX_LEVELS: [(i64, i64); 4] = [(50000, 5), (100000, 4), (300000, 3), (500000, 2)];
pub const DEFAULT_KEY: &[u8] = b"cucumber";
pub const KEY_CHECK: &[u8] = b"cucumbers everywhere";

/// One TL string (length prefix, payload, zero padding) off the front of `rest`.
pub fn take_tl_string<'a>(rest: &mut &'a [u8]) -> Option<&'a [u8]> {
    let first = *rest.first()?;
    let (len, prefix) = match first {
        0..=253 => (first as usize, 1),
        254 => {
            let bytes = rest.get(1..4)?;
            (bytes[0] as usize | (bytes[1] as usize) << 8 | (bytes[2] as usize) << 16, 4)
        }
        _ => return None,
    };
    let start = prefix;
    let end = start + len;
    if end > rest.len() {
        return None;
    }
    let padded = end + (4 - end % 4) % 4;
    if padded > rest.len() {
        return None;
    }
    let value = &rest[start..end];
    *rest = &rest[padded..];
    Some(value)
}

/// `TlStorerUnsafe::store_string`: 1-byte length below 254, else a 4-byte
/// 254/255 form, then the payload, then zero padding to a multiple of 4.
pub fn store_tl_string(payload: &[u8]) -> Vec<u8> {
    let len = payload.len();
    let mut out = Vec::with_capacity(len + 8);
    if len < 254 {
        out.push(len as u8);
    } else if len < (1 << 24) {
        out.push(254);
        out.push((len & 255) as u8);
        out.push(((len >> 8) & 255) as u8);
        out.push(((len >> 16) & 255) as u8);
    } else {
        out.push(255);
        out.push((len & 255) as u8);
        out.push(((len >> 8) & 255) as u8);
        out.push(((len >> 16) & 255) as u8);
        out.push(((len >> 24) & 255) as u8);
        out.push(0);
        out.push(0);
        out.push(0);
    }
    out.extend_from_slice(payload);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text == "-" {
        // "-" is the empty byte string, so a zero-length token stays a token
        return Some(Vec::new());
    }
    if text.len() % 2 != 0 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    for chunk in bytes.chunks(2) {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

const fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

static CRC32_TABLE: [u32; 256] = crc32_table();

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc = CRC32_TABLE[((crc ^ byte as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

pub fn create_raw(id: u64, event_type: i32, flags: i32, data: &[u8]) -> Vec<u8> {
    let size = (MIN_SIZE + data.len()) as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&event_type.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(data);
    let crc = crc32(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

pub fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

pub fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(raw)
}