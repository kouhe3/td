//! Dump a TDLib binlog the same way the repository's `binlog_dump` does.
//!
//! Frame layout (tddb/td/db/binlog/BinlogEvent.h:52-54):
//!   [size:u32][id:u64][type:i32][flags:i32][extra:u64][data:size-32][crc32:u32]
//!
//! The first frame is a plaintext service event `type = -3` (AesCtrEncryption);
//! everything after it is AES-256-CTR ciphertext. The key is
//! `PBKDF2-HMAC-SHA256("cucumber", key_salt, 2, 32)` (the default raw DbKey at
//! td/telegram/TdDb.cpp:673), the IV comes from the event, and
//! `HMAC-SHA256(key, "cucumbers everywhere")` must equal the stored key hash.
//!
//! Output matches tddb/td/db/binlog/binlog_dump.cpp byte for byte:
//!   LogEvent[[event_id:0x…][type:…][flags:…][size:…][data:…]]
//!   [handler:0x…][full_size:…][compressed_size:…]
//!   TOTAL: …            (per-handler key trie, then its compressed variant)
//!
//! `full` counts every frame. `compressed` counts what the processor keeps:
//! a `Rewrite` frame (flags 1) replaces an earlier frame with the same id,
//! service frames (type < 0) never enter, everything else is appended
//! (tddb/td/db/binlog/detail/BinlogEventsProcessor.cpp:17-58).
//!
//! Usage: binlog_head <binlog_file>

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::process::ExitCode;

use aes::cipher::{KeyIvInit, StreamCipher};
use hmac::{Hmac, Mac};
use sha2::Sha256;

#[allow(dead_code)]
#[path = "../binlog_format.rs"]
mod binlog_format;

use binlog_format::{u32_at, DEFAULT_KEY, FLAG_REWRITE, HEADER_SIZE, KEY_CHECK, MIN_SIZE, TAIL_SIZE};

const AES_CTR_ENCRYPTION: i32 = -3;
const CONFIG_PMC_MAGIC: i32 = 0x1f18;
const BINLOG_PMC_MAGIC: i32 = 0x4327;

type Aes256Ctr = ctr::Ctr128BE<aes::Aes256>;

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

/// `format::escaped` (tdutils/td/utils/format.h:141): printable ASCII except
/// `"` and `\` stays literal, everything else becomes a three-digit octal escape.
fn push_escaped(data: &[u8], out: &mut String) {
    for &c in data {
        if c > 31 && c < 127 && c != b'"' && c != b'\\' {
            out.push(c as char);
        } else {
            out.push('\\');
            out.push((b'0' + (c >> 6)) as char);
            out.push((b'0' + ((c >> 3) & 7)) as char);
            out.push((b'0' + (c & 7)) as char);
        }
    }
}

/// `format::as_size` (tdutils/td/utils/format.h:185): integers, promoted at 100000.
fn as_size(size: u64) -> String {
    const UNITS: [(&str, u64); 4] = [("B", 1), ("KB", 1 << 10), ("MB", 1 << 20), ("GB", 1 << 30)];
    let mut i = 0;
    while i + 1 < UNITS.len() && size >= 100_000 * UNITS[i].1 {
        i += 1;
    }
    format!("{}{}", size / UNITS[i].1, UNITS[i].0)
}

#[derive(Clone)]
struct Node {
    sum: u64,
    next: [u32; 256],
}

/// Prefix trie over event keys, used for the per-handler summary
/// (tddb/td/db/binlog/binlog_dump.cpp:22-86).
struct Trie {
    nodes: Vec<Node>,
}

impl Trie {
    fn new() -> Self {
        Self {
            nodes: vec![Node {
                sum: 0,
                next: [0; 256],
            }],
        }
    }

    /// Keys are stored with a trailing NUL, as in `Trie::add`.
    fn add(&mut self, key: &[u8]) {
        let mut v = 0usize;
        self.nodes[v].sum += 1;
        for &c in key.iter().chain(std::iter::once(&0u8)) {
            let idx = self.nodes[v].next[c as usize];
            let child = if idx == 0 {
                let new = self.nodes.len() as u32;
                self.nodes.push(Node {
                    sum: 0,
                    next: [0; 256],
                });
                self.nodes[v].next[c as usize] = new;
                new
            } else {
                idx
            } as usize;
            v = child;
            self.nodes[v].sum += 1;
        }
    }

    fn dump(&self, out: &mut String) {
        if self.nodes[0].sum == 0 {
            return;
        }
        out.push_str(&format!("TOTAL: {}\n", self.nodes[0].sum));
        self.do_dump(Vec::new(), 0, out);
    }

    fn do_dump(&self, mut path: Vec<u8>, v: usize, out: &mut String) {
        let is_word_end = path.last() == Some(&0);
        let mut need_stop = false;
        let mut next_count = 0;
        for c in 0..256usize {
            if self.nodes[v].next[c] != 0 {
                let alpha = (c as u8 as char).is_ascii_alphabetic();
                need_stop |= c >= 128 || !(alpha || c == b'.' as usize || c == b'_' as usize);
                next_count += 1;
            }
        }
        need_stop |= next_count == 0 || (next_count >= 2 && self.nodes[v].sum <= self.nodes[0].sum / 100);

        if is_word_end || need_stop {
            if is_word_end {
                path.pop();
            } else if next_count != 1 || self.nodes[v].next[0] == 0 {
                path.push(b'*');
            }
            let pct = self.nodes[v].sum as f64 * 100.0 / self.nodes[0].sum as f64;
            out.push_str(&format!("{} {pct:.2}% [", self.nodes[v].sum));
            push_escaped(&path, out);
            out.push_str("]\n");
            return;
        }
        for c in 0..256usize {
            let next = self.nodes[v].next[c];
            if next != 0 {
                let mut child_path = path.clone();
                child_path.push(c as u8);
                self.do_dump(child_path, next as usize, out);
            }
        }
    }
}

/// Mirrors `BinlogEventsProcessor`: the kept set is keyed by event id, so a
/// frame carrying the `Rewrite` flag replaces its predecessor in place.
#[derive(Default)]
struct Processor {
    slots: Vec<Slot>,
    index: HashMap<u64, usize>,
    last_id: Option<u64>,
}

struct Slot {
    ty: i32,
    raw: u64,
    key: Option<Vec<u8>>,
}

impl Processor {
    fn add(&mut self, id: u64, ty: i32, flags: i32, raw: u64, key: Option<Vec<u8>>) {
        let fixed = id * 2;
        let can_rewrite = flags & FLAG_REWRITE != 0
            && self
                .last_id
                .map(|last| last * 2 >= fixed)
                .unwrap_or(false);
        if can_rewrite {
            if let Some(&pos) = self.index.get(&id) {
                self.slots[pos] = Slot { ty, raw, key };
            }
            return;
        }
        if ty < 0 {
            return;
        }
        self.index.insert(id, self.slots.len());
        self.slots.push(Slot { ty, raw, key });
        self.last_id = Some(id);
    }
}

#[derive(Default)]
struct Summary {
    full_size: u64,
    compressed_size: u64,
    full_trie: Option<Trie>,
    compressed_trie: Option<Trie>,
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

fn key_of(ty: i32, data: &[u8]) -> Option<Vec<u8>> {
    if ty != CONFIG_PMC_MAGIC && ty != BINLOG_PMC_MAGIC {
        return None;
    }
    let mut off = 0;
    tl_string(data, &mut off).ok().map(|k| k.to_vec())
}

fn run(data: &[u8]) -> Result<usize, String> {
    if data.len() < MIN_SIZE {
        return Err("file shorter than one frame".to_string());
    }
    let size0 = u32_at(data, 0) as usize;
    if size0 < MIN_SIZE || size0 > data.len() {
        return Err(format!("bad first frame size {size0}"));
    }
    let text = &mut String::new();
    let mut full: BTreeMap<u64, Summary> = BTreeMap::new();
    let mut processor = Processor::default();

    let type0 = i32_at(data, 12);
    let flags0 = i32_at(data, 16);
    let data0 = &data[HEADER_SIZE..size0 - TAIL_SIZE];
    emit(text, u64::from_le_bytes(data[4..12].try_into().unwrap()), type0, flags0, data0);
    let key0 = key_of(type0, data0);
    count_full(&mut full, type0, size0 as u64, key0.as_deref());
    processor.add(
        u64::from_le_bytes(data[4..12].try_into().unwrap()),
        type0,
        flags0,
        size0 as u64,
        key0,
    );

    let mut events = 1usize;
    if type0 == AES_CTR_ENCRYPTION {
        let mut off = 4; // skip the stored flags word
        let salt = tl_string(data0, &mut off)?;
        let iv = tl_string(data0, &mut off)?;
        let key_hash = tl_string(data0, &mut off)?;

        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(DEFAULT_KEY, salt, 2, &mut key);
        let mut mac = Hmac::<Sha256>::new_from_slice(&key).map_err(|e| e.to_string())?;
        mac.update(KEY_CHECK);
        mac.verify_slice(key_hash)
            .map_err(|_| "key hash mismatch: wrong binlog key".to_string())?;

        let mut rest = data[size0..].to_vec();
        let mut stream = Aes256Ctr::new_from_slices(&key, iv).map_err(|e| e.to_string())?;
        stream.apply_keystream(&mut rest);
        events += frames(&rest, size0, text, &mut full, &mut processor)?;
    } else {
        events += frames(&data[size0..], size0, text, &mut full, &mut processor)?;
    }

    // The compressed side is the processor's final view, not the frame stream.
    for slot in &processor.slots {
        full.entry(0).or_default().compressed_size += slot.raw;
        let entry = full.entry(slot.ty as i64 as u64).or_default();
        entry.compressed_size += slot.raw;
        if let Some(k) = &slot.key {
            entry
                .compressed_trie
                .get_or_insert_with(Trie::new)
                .add(k);
        }
    }

    for (ty, info) in &full {
        text.push_str(&format!(
            "[handler:0x{ty:016x}][full_size:{}][compressed_size:{}]\n",
            as_size(info.full_size),
            as_size(info.compressed_size)
        ));
        if let Some(trie) = &info.full_trie {
            trie.dump(text);
        }
        if info.full_size != info.compressed_size {
            if let Some(trie) = &info.compressed_trie {
                trie.dump(text);
            }
        }
    }
    print!("{text}");
    Ok(events)
}

fn emit(text: &mut String, id: u64, ty: i32, flags: i32, data: &[u8]) {
    text.push_str(&format!(
        "LogEvent[[event_id:0x{id:016x}][type:{ty}][flags:{flags}][size:{}][data:",
        data.len()
    ));
    push_escaped(data, text);
    text.push_str("]]\n");
}

fn count_full(full: &mut BTreeMap<u64, Summary>, ty: i32, raw: u64, key: Option<&[u8]>) {
    full.entry(0).or_default().full_size += raw;
    let entry = full.entry(ty as i64 as u64).or_default();
    entry.full_size += raw;
    if let Some(k) = key {
        entry.full_trie.get_or_insert_with(Trie::new).add(k);
    }
}

fn frames(
    buf: &[u8],
    file_offset: usize,
    text: &mut String,
    full: &mut BTreeMap<u64, Summary>,
    processor: &mut Processor,
) -> Result<usize, String> {
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
        let ty = i32_at(buf, off + 12);
        let flags = i32_at(buf, off + 16);
        let data = &buf[off + HEADER_SIZE..off + size - TAIL_SIZE];
        emit(text, id, ty, flags, data);
        let key = key_of(ty, data);
        count_full(full, ty, size as u64, key.as_deref());
        processor.add(id, ty, flags, size as u64, key);
        off += size;
        count += 1;
    }
    if off != buf.len() {
        return Err(format!("trailing {} bytes at +{off}", buf.len() - off));
    }
    Ok(count)
}