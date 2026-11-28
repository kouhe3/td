//! Judge `td::BinlogKeyValue<td::Binlog>` (tddb/td/db/BinlogKeyValue.h) and
//! `td::SeqKeyValue` (tddb/td/db/SeqKeyValue.h) against the C++ oracle
//! (tools/fixtures/binlog_kv_probe.cpp).
//!
//!   binlog_kv_probe <script.txt> <out_binlog>
//!
//! The framing, `TlStorerUnsafe::store_string`, `td::crc32`, the frame walk and
//! `detail::BinlogEventsProcessor` are copied from the committed write lane
//! (`rust/src/bin/binlog_write_probe.rs`, itself judged byte-exact against the
//! same tddb code) and are cited there; this file only adds the key-value layer
//! and drops encryption, which the KV lane does not use (`DbKey::empty()`).
//!
//! What the KV layer does (measured; docs/facts.md F191/F192):
//!   * the map holds `(value, event_id)`; a `set` of an existing key with an
//!     unchanged value returns 0 and writes nothing;
//!   * a changed value writes `create_raw(previous_event_id, magic, Rewrite,
//!     [key][value] TL strings)` and returns the new sequence number -- the frame
//!     id is the *previous* event id, not the sequence number;
//!   * only a first-time `set` writes `id = seq_no` (there is no previous id);
//!   * `erase` writes `create_raw(previous_event_id, -2 /*Empty*/, Rewrite, "")`
//!     -- 32 bytes with no data at all, which is how an erase differs from a
//!     `set` to an empty value (that one still carries two TL strings, >= 8
//!     bytes) even though `get()` cannot tell them apart (`isset` can);
//!   * `erase_batch` returns `last + 1` (the counter is untouched by
//!     `next_event_id(0)`) and then burns one id per erased key while writing the
//!     frames in the caller's order;
//!   * `erase_by_prefix` uses `next_event_id(count)` -- the counter advances by
//!     the match count -- and leaves the returned number unused;
//!   * `init` replays every *live* event into the map with no type/magic filter
//!     (that is TdDb's own switch, td/telegram/TdDb.cpp:170-181), and the
//!     sequence counter is rebuilt from the replay, so it can jump backwards
//!     across a reopen;
//!   * `init` starts with `close()` and `close()` is `*this = BinlogKeyValue()`,
//!     so every init without an override resets `magic_` to MAGIC (0x2a280000)
//!     and drops the map; only `init(..., override_magic = n)` installs an
//!     override, and it lasts exactly until the next `reopen`;
//!   * `set` of an empty key is `CHECK(!key.empty())`: the script prints `reject`
//!     and both sides agree instead of dying;
//!   * an open Binlog holds a whole-file write lock, so `dumpfile` is only legal
//!     while the KV is closed (`reject dumpfile reason=open` otherwise).
//!
//! Narration is byte-identical to tools/fixtures/binlog_kv_probe.cpp; see that
//! file for the full command list. `raw` appends a frame through a standalone
//! writer while the KV is closed, which is how a foreign event type gets into a
//! file the KV will then load.

use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::process::ExitCode;

use sha2::{Digest, Sha256};

const HEADER_SIZE: usize = 4 + 8 + 4 + 4 + 8;
const TAIL_SIZE: usize = 4;
const MIN_SIZE: usize = HEADER_SIZE + TAIL_SIZE;
const FLAG_REWRITE: i32 = 1;
const SERVICE_EMPTY: i32 = -2;
const LAZY_FLUSH_THRESHOLD: usize = 1 << 14;
const HEX_PREVIEW: usize = 32;
/// `BinlogKeyValue::MAGIC`
const MAGIC: i32 = 0x2a28_0000;
/// `Binlog::add_event`: (min file size, dead-ratio divisor)
const REINDEX_LEVELS: [(i64, i64); 4] = [(50000, 5), (100000, 4), (300000, 3), (500000, 2)];

fn to_hex(data: &[u8], limit: usize) -> String {
    let shown = data.len().min(limit);
    let mut out = String::with_capacity(shown * 2 + 2);
    for byte in &data[..shown] {
        out.push_str(&format!("{byte:02x}"));
    }
    if shown < data.len() {
        out.push_str("..");
    }
    if out.is_empty() {
        out = "-".to_owned();
    }
    out
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
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

/// `TlStorerUnsafe::store_string`: 1-byte length below 254, else a 4-byte
/// 254/255 form, then the payload, then zero padding to a multiple of 4.
fn store_tl_string(payload: &[u8]) -> Vec<u8> {
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

/// One TL string (length prefix, payload, zero padding) off the front of `rest`.
fn take_tl_string<'a>(rest: &mut &'a [u8]) -> Option<&'a [u8]> {
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

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc = CRC32_TABLE[((crc ^ byte as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

fn create_raw(id: u64, event_type: i32, flags: i32, data: &[u8]) -> Vec<u8> {
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

/// A frame plus the parsed fields, exactly as `BinlogEvent` holds them.
#[derive(Clone)]
struct Frame {
    id: u64,
    event_type: i32,
    flags: i32,
    data: Vec<u8>,
    raw: Vec<u8>,
}

fn make_event(id: u64, event_type: i32, flags: i32, data: &[u8]) -> Frame {
    Frame {
        id,
        event_type,
        flags,
        data: data.to_vec(),
        raw: create_raw(id, event_type, flags, data),
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(raw)
}

/// Frame walk of a plaintext binlog; stops at the first incomplete frame.
fn walk_frames(bytes: &[u8]) -> Vec<Frame> {
    let mut events = Vec::new();
    let mut offset = 0;
    while offset + 4 <= bytes.len() {
        let size = u32_at(bytes, offset) as usize;
        if size < MIN_SIZE || offset + size > bytes.len() {
            break;
        }
        events.push(Frame {
            id: u64_at(bytes, offset + 4),
            event_type: u32_at(bytes, offset + 12) as i32,
            flags: u32_at(bytes, offset + 16) as i32,
            data: bytes[offset + HEADER_SIZE..offset + size - TAIL_SIZE].to_vec(),
            raw: bytes[offset..offset + size].to_vec(),
        });
        offset += size;
    }
    events
}

struct Entry {
    /// `event_id * 2 + was_deleted`, the processor's ordering key
    fixed: u64,
    event: Frame,
    deleted: bool,
}

/// `detail::BinlogEventsProcessor`: the live set the replay keeps, plus the
/// counters that decide when the writer reindexes.
#[derive(Default)]
struct Processor {
    entries: Vec<Entry>,
    total_events: usize,
    empty_events: usize,
    last_event_id: u64,
    total_raw: usize,
}

impl Processor {
    fn back(&self) -> Option<u64> {
        self.entries.last().map(|entry| entry.fixed)
    }

    /// `BinlogEventsProcessor::do_event`.
    fn add(&mut self, event: Frame) -> Result<(), String> {
        let fixed = event.id.wrapping_mul(2);
        let is_rewrite = event.flags & FLAG_REWRITE != 0;
        if is_rewrite && self.back().is_some_and(|back| back >= fixed) {
            let position = self
                .entries
                .iter()
                .position(|entry| entry.fixed == fixed)
                .ok_or_else(|| format!("Ignore rewrite log event id={} type={}", event.id, event.event_type))?;
            self.total_raw -= self.entries[position].event.raw.len();
            if event.event_type == SERVICE_EMPTY {
                self.entries[position].fixed = fixed + 1;
                self.entries[position].deleted = true;
                self.empty_events += 1;
            } else {
                // the folded copy loses the flag, but `raw` keeps the written bytes
                let mut folded = event;
                folded.flags &= !FLAG_REWRITE;
                self.total_raw += folded.raw.len();
                self.entries[position] = Entry {
                    fixed,
                    event: folded,
                    deleted: false,
                };
            }
        } else if event.event_type < 0 {
            // service frames are written but never replayed
        } else {
            if self.back().is_some_and(|back| back >= fixed) {
                return Err(format!("out of order event id={}", event.id));
            }
            self.last_event_id = event.id;
            self.total_raw += event.raw.len();
            self.total_events += 1;
            self.entries.push(Entry {
                fixed,
                event,
                deleted: false,
            });
        }
        if self.total_events > 10 && self.empty_events * 4 > self.total_events * 3 {
            self.compactify();
        }
        Ok(())
    }

    fn compactify(&mut self) {
        self.entries.retain(|entry| !entry.deleted);
        self.total_events = self.entries.len();
        self.empty_events = 0;
    }

    fn live(&self) -> impl Iterator<Item = &Frame> {
        self.entries.iter().filter(|entry| !entry.deleted).map(|entry| &entry.event)
    }
}

/// The plaintext binlog writer the KV sits on (`Binlog` minus encryption).
struct Writer {
    path: String,
    file: Option<File>,
    buffer: Vec<u8>,
    processor: Processor,
    /// `Binlog::last_event_id_`; only a reopen resynchronises it from the replay
    last_event_id: u64,
    /// all bytes of the current file generation, buffered ones included
    fd_size: usize,
    need_sync: bool,
}

impl Writer {
    /// `Binlog::init`: `fresh` destroys the file first (`Binlog::destroy`).
    fn open(path: &str, fresh: bool) -> Result<Self, String> {
        if fresh {
            let _ = fs::remove_file(format!("{path}.new"));
            let _ = fs::remove_file(path);
        }
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| format!("open {path}: {error}"))?;
        let mut writer = Writer {
            path: path.to_owned(),
            file: Some(file),
            buffer: Vec::new(),
            processor: Processor::default(),
            last_event_id: 0,
            fd_size: 0,
            need_sync: false,
        };
        writer.load_file()?;
        Ok(writer)
    }

    fn next_event_id(&mut self) -> u64 {
        self.last_event_id += 1;
        self.last_event_id
    }

    /// `Binlog::next_event_id(int32 shift)`.
    fn next_event_id_shift(&mut self, shift: i32) -> u64 {
        let result = self.last_event_id + 1;
        self.last_event_id += shift as u64;
        result
    }

    fn load_file(&mut self) -> Result<(), String> {
        let bytes = fs::read(&self.path).unwrap_or_default();
        let mut processor = Processor::default();
        for event in walk_frames(&bytes) {
            processor.add(event)?;
        }
        self.buffer.clear();
        self.last_event_id = processor.last_event_id;
        self.processor = processor;
        self.fd_size = bytes.len();
        self.file
            .as_mut()
            .ok_or_else(|| "load without an open binlog".to_owned())?
            .seek(SeekFrom::End(0))
            .map_err(|error| format!("seek: {error}"))?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let chunk = std::mem::take(&mut self.buffer);
        let file = self.file.as_mut().ok_or_else(|| "flush without an open binlog".to_owned())?;
        file.write_all(&chunk).map_err(|error| format!("write: {error}"))?;
        self.need_sync = true;
        Ok(())
    }

    fn sync(&mut self) -> Result<(), String> {
        self.flush()?;
        if self.need_sync {
            if let Some(file) = self.file.as_mut() {
                file.sync_all().map_err(|error| format!("sync: {error}"))?;
            }
            self.need_sync = false;
        }
        Ok(())
    }

    fn lazy_flush(&mut self) -> Result<(), String> {
        if self.buffer.len() > LAZY_FLUSH_THRESHOLD {
            self.flush()?;
        }
        Ok(())
    }

    fn close(&mut self) -> Result<(), String> {
        if self.file.is_none() {
            return Ok(());
        }
        self.sync()?;
        self.file = None;
        Ok(())
    }

    /// `Binlog::add_event`: buffer the frame, feed the processor, then let the
    /// dead-ratio rule decide whether the whole file gets regenerated.
    fn add_frame(&mut self, event: Frame) -> Result<(), String> {
        self.buffer.extend_from_slice(&event.raw);
        self.fd_size += event.raw.len();
        self.processor.add(event)?;
        self.lazy_flush()?;
        let fd_size = self.fd_size as i64;
        let total = self.processor.total_raw as i64;
        let reindex = REINDEX_LEVELS
            .iter()
            .any(|&(min_size, rate)| fd_size > min_size && fd_size / rate > total);
        if reindex {
            self.write_generation()?;
        }
        Ok(())
    }

    /// `Binlog::do_reindex` with an empty key: rewrite `path` as the live set.
    fn write_generation(&mut self) -> Result<(), String> {
        self.flush()?;
        let mut out = Vec::with_capacity(self.processor.total_raw);
        for event in self.processor.live() {
            out.extend_from_slice(&event.raw);
        }
        let file = self.file.as_mut().ok_or_else(|| "reindex without an open binlog".to_owned())?;
        file.set_len(0).map_err(|error| format!("truncate: {error}"))?;
        file.seek(SeekFrom::Start(0)).map_err(|error| format!("seek: {error}"))?;
        file.write_all(&out).map_err(|error| format!("write: {error}"))?;
        self.need_sync = true;
        self.fd_size = out.len();
        Ok(())
    }
}

/// `BinlogKeyValue::Event::parse` plus `fetch_end`.
fn parse_kv_event(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut rest = data;
    let key = take_tl_string(&mut rest).ok_or_else(|| "bad KV event key".to_owned())?;
    let value = take_tl_string(&mut rest).ok_or_else(|| "bad KV event value".to_owned())?;
    if !rest.is_empty() {
        return Err(format!("{} trailing bytes in KV event", rest.len()));
    }
    Ok((key.to_vec(), value.to_vec()))
}

/// `BinlogKeyValue<Binlog>`: the map keeps `(value, event_id)`.
struct Kv {
    path: String,
    writer: Option<Writer>,
    map: BTreeMap<Vec<u8>, (Vec<u8>, u64)>,
    magic: i32,
}

impl Kv {
    fn new(path: &str) -> Self {
        Kv {
            path: path.to_owned(),
            writer: None,
            map: BTreeMap::new(),
            magic: MAGIC,
        }
    }

    fn is_open(&self) -> bool {
        self.writer.is_some()
    }

    fn size(&self) -> usize {
        self.map.len()
    }

    /// `BinlogKeyValue::init`: `close()` (which also resets `magic_` and drops the
    /// map), then an optional override, then a fresh Binlog and a replay.
    fn init(&mut self, fresh: bool, override_magic: i32) -> Result<(), String> {
        self.close()?;
        if override_magic != 0 {
            self.magic = override_magic;
        }
        let mut writer = Writer::open(&self.path, fresh)?;
        let live: Vec<Frame> = writer.processor.live().cloned().collect();
        for event in live {
            let (key, value) = parse_kv_event(&event.data)?;
            if key.is_empty() {
                // LOG(ERROR) << "Have event with empty key";
                continue;
            }
            self.map.insert(key, (value, event.id));
        }
        writer.buffer.clear();
        self.writer = Some(writer);
        Ok(())
    }

    fn close(&mut self) -> Result<(), String> {
        if let Some(writer) = self.writer.as_mut() {
            writer.close()?;
        }
        *self = Kv::new(&self.path.clone());
        Ok(())
    }

    fn sync(&mut self) -> Result<(), String> {
        self.writer
            .as_mut()
            .ok_or_else(|| "sync without an open binlog".to_owned())?
            .sync()
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<u64, String> {
        let (stored, previous_id) = self.map.get(key).cloned().unwrap_or_default();
        if previous_id != 0 && stored == value {
            return Ok(0);
        }
        let seq_no = self.writer_mut()?.next_event_id();
        let rewrite = previous_id != 0;
        let event_id = if rewrite { previous_id } else { seq_no };
        self.map.insert(key.to_vec(), (value.to_vec(), event_id));
        let mut data = store_tl_string(key);
        data.extend_from_slice(&store_tl_string(value));
        let flags = if rewrite { FLAG_REWRITE } else { 0 };
        let event = make_event(event_id, self.magic, flags, &data);
        self.writer_mut()?.add_frame(event)?;
        Ok(seq_no)
    }

    fn erase(&mut self, key: &[u8]) -> Result<u64, String> {
        let Some((_, event_id)) = self.map.remove(key) else {
            return Ok(0);
        };
        let seq_no = self.writer_mut()?.next_event_id();
        let event = make_event(event_id, SERVICE_EMPTY, FLAG_REWRITE, &[]);
        self.writer_mut()?.add_frame(event)?;
        Ok(seq_no)
    }

    /// `BinlogKeyValue::erase_batch` -> `Binlog::erase_batch`: the returned
    /// number is `last + 1` and one id is burned per erased key.
    fn erase_batch(&mut self, keys: &[Vec<u8>]) -> Result<u64, String> {
        let mut event_ids = Vec::new();
        for key in keys {
            if let Some((_, event_id)) = self.map.remove(key) {
                event_ids.push(event_id);
            }
        }
        if event_ids.is_empty() {
            return Ok(0);
        }
        let seq_no = self.writer_mut()?.next_event_id_shift(0);
        for event_id in event_ids {
            self.writer_mut()?.next_event_id();
            let event = make_event(event_id, SERVICE_EMPTY, FLAG_REWRITE, &[]);
            self.writer_mut()?.add_frame(event)?;
        }
        Ok(seq_no)
    }

    /// `BinlogKeyValue::erase_by_prefix`: `next_event_id(count)` moves the
    /// counter by the match count, then one erase frame per removed key.
    fn erase_by_prefix(&mut self, prefix: &[u8]) -> Result<usize, String> {
        let mut event_ids = Vec::new();
        let keys: Vec<Vec<u8>> = self
            .map
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect();
        for key in keys {
            if let Some((_, event_id)) = self.map.remove(&key) {
                event_ids.push(event_id);
            }
        }
        let removed = event_ids.len();
        self.writer_mut()?.next_event_id_shift(removed as i32);
        for event_id in event_ids {
            let event = make_event(event_id, SERVICE_EMPTY, FLAG_REWRITE, &[]);
            self.writer_mut()?.add_frame(event)?;
        }
        Ok(removed)
    }

    fn get(&self, key: &[u8]) -> Vec<u8> {
        self.map.get(key).map(|entry| entry.0.clone()).unwrap_or_default()
    }

    fn isset(&self, key: &[u8]) -> bool {
        self.map.contains_key(key)
    }

    fn get_all(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.map.iter().map(|(key, entry)| (key.clone(), entry.0.clone())).collect()
    }

    fn prefix_get(&self, prefix: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.map
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, entry)| (key[prefix.len()..].to_vec(), entry.0.clone()))
            .collect()
    }

    fn writer_mut(&mut self) -> Result<&mut Writer, String> {
        self.writer.as_mut().ok_or_else(|| "no open binlog".to_owned())
    }
}

/// `SeqKeyValue`: memory only, every mutation that changes something burns one
/// id except `erase_batch`, which burns the count and returns `current_id_ + 1`.
#[derive(Default)]
struct SeqKv {
    map: BTreeMap<Vec<u8>, Vec<u8>>,
    current_id: u64,
}

impl SeqKv {
    fn next_seq_no(&mut self) -> u64 {
        self.current_id += 1;
        self.current_id
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> u64 {
        match self.map.get(key) {
            Some(stored) if stored.as_slice() == value => 0,
            _ => {
                self.map.insert(key.to_vec(), value.to_vec());
                self.next_seq_no()
            }
        }
    }

    fn erase(&mut self, key: &[u8]) -> u64 {
        if self.map.remove(key).is_none() {
            return 0;
        }
        self.next_seq_no()
    }

    fn erase_batch(&mut self, keys: &[Vec<u8>]) -> u64 {
        let mut count = 0u64;
        for key in keys {
            if self.map.remove(key).is_some() {
                count += 1;
            }
        }
        if count == 0 {
            return 0;
        }
        let result = self.current_id + 1;
        self.current_id += count;
        result
    }

    fn seq_no(&self) -> u64 {
        self.current_id + 1
    }
}

fn print_pairs(pairs: &[(Vec<u8>, Vec<u8>)]) {
    for (key, value) in pairs {
        print!(" {}={}", to_hex(key, key.len()), to_hex(value, HEX_PREVIEW));
    }
}

fn fail(what: &str, line: &str) -> ExitCode {
    eprintln!("binlog_kv_probe: {what} in line {line}");
    ExitCode::from(2)
}

fn probe_error(error: &str) -> ExitCode {
    eprintln!("binlog_kv_probe: {error}");
    ExitCode::from(2)
}

fn script_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim_end())
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| line.trim_start().to_owned())
        .collect()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(data) {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: binlog_kv_probe <script.txt> <out_binlog>");
        return ExitCode::from(2);
    }
    let path = args[2].clone();
    let script = match fs::read_to_string(&args[1]) {
        Ok(script) => script,
        Err(error) => {
            eprintln!("binlog_kv_probe: cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    let mut lines = script_lines(&script);

    let mut kv = Kv::new(&path);
    let mut memory = SeqKv::default();
    let mut last_seq: u64 = 0;

    let attach = lines.first().is_some_and(|line| line == "attach");
    if attach {
        let existed = fs::metadata(&path).is_ok();
        if let Err(error) = kv.init(false, 0) {
            eprintln!("binlog_kv_probe: init failed: {error}");
            return ExitCode::from(2);
        }
        println!("attach ok={} size={}", i32::from(existed), kv.size());
        lines.remove(0);
    } else if let Err(error) = kv.init(true, 0) {
        eprintln!("binlog_kv_probe: init failed: {error}");
        return ExitCode::from(2);
    }

    for line in &lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        let command = words[0];

        match command {
            "magic" => {
                if words.len() != 2 {
                    return fail("bad magic", line);
                }
                let magic: i32 = match words[1].parse() {
                    Ok(value) => value,
                    Err(_) => return fail("bad magic number", line),
                };
                let fresh = false;
                if let Err(error) = kv.init(fresh, magic) {
                    eprintln!("binlog_kv_probe: init failed: {error}");
                    return ExitCode::from(2);
                }
                println!("magic {magic} ok=1 size={}", kv.size());
            }
            "set" => {
                if words.len() != 3 {
                    return fail("bad set", line);
                }
                let (key, value) = match (from_hex(words[1]), from_hex(words[2])) {
                    (Some(key), Some(value)) => (key, value),
                    _ => return fail("bad set hex", line),
                };
                if key.is_empty() {
                    println!("reject set key=- reason=empty-key");
                    continue;
                }
                if !kv.is_open() {
                    println!("reject set reason=closed");
                    continue;
                }
                let seq = match kv.set(&key, &value) {
                    Ok(seq) => seq,
                    Err(error) => return probe_error(&error),
                };
                last_seq = seq;
                println!(
                    "set key={} value={} vlen={} seq={seq}",
                    to_hex(&key, key.len()),
                    to_hex(&value, HEX_PREVIEW),
                    value.len()
                );
            }
            "erase" => {
                if words.len() != 2 {
                    return fail("bad erase", line);
                }
                let Some(key) = from_hex(words[1]) else {
                    return fail("bad erase hex", line);
                };
                if !kv.is_open() {
                    println!("reject erase reason=closed");
                    continue;
                }
                let seq = match kv.erase(&key) {
                    Ok(seq) => seq,
                    Err(error) => return probe_error(&error),
                };
                last_seq = seq;
                println!("erase key={} seq={seq}", to_hex(&key, key.len()));
            }
            "erase_batch" => {
                if words.len() < 2 {
                    return fail("bad erase_batch", line);
                }
                let mut keys = Vec::new();
                for word in &words[1..] {
                    match from_hex(word) {
                        Some(key) => keys.push(key),
                        None => return fail("bad erase_batch hex", line),
                    }
                }
                if !kv.is_open() {
                    println!("reject erasebatch reason=closed");
                    continue;
                }
                let seq = match kv.erase_batch(&keys) {
                    Ok(seq) => seq,
                    Err(error) => return probe_error(&error),
                };
                last_seq = seq;
                println!("erasebatch keys={} seq={seq}", keys.len());
            }
            "eraseprefix" => {
                if words.len() != 2 {
                    return fail("bad eraseprefix", line);
                }
                let Some(prefix) = from_hex(words[1]) else {
                    return fail("bad eraseprefix hex", line);
                };
                if !kv.is_open() {
                    println!("reject eraseprefix reason=closed");
                    continue;
                }
                let removed = match kv.erase_by_prefix(&prefix) {
                    Ok(removed) => removed,
                    Err(error) => return probe_error(&error),
                };
                println!("eraseprefix p={} removed={removed}", to_hex(&prefix, prefix.len()));
            }
            "get" | "isset" => {
                if words.len() != 2 {
                    return fail("bad key command", line);
                }
                let Some(key) = from_hex(words[1]) else {
                    return fail("bad key hex", line);
                };
                if !kv.is_open() {
                    println!("reject {command} reason=closed");
                    continue;
                }
                let key_text = to_hex(&key, key.len());
                if command == "get" {
                    let value = kv.get(&key);
                    println!(
                        "get key={key_text} value={} vlen={}",
                        to_hex(&value, HEX_PREVIEW),
                        value.len()
                    );
                } else {
                    println!("isset key={key_text} value={}", i32::from(kv.isset(&key)));
                }
            }
            "getall" | "foreach" | "prefix" => {
                let prefix = if command == "prefix" {
                    if words.len() != 2 {
                        return fail("bad prefix", line);
                    }
                    match from_hex(words[1]) {
                        Some(prefix) => prefix,
                        None => return fail("bad prefix hex", line),
                    }
                } else {
                    if words.len() != 1 {
                        return fail("bad command", line);
                    }
                    Vec::new()
                };
                if !kv.is_open() {
                    println!("reject {command} reason=closed");
                    continue;
                }
                let pairs = if command == "prefix" {
                    kv.prefix_get(&prefix)
                } else {
                    kv.get_all()
                };
                if command == "getall" {
                    print!("getall n={}", pairs.len());
                } else if command == "prefix" {
                    print!("prefix p={} n={}", to_hex(&prefix, prefix.len()), pairs.len());
                } else {
                    print!("foreach n={}", pairs.len());
                }
                print_pairs(&pairs);
                println!();
            }
            "seq" => {
                if words.len() != 1 {
                    return fail("bad seq", line);
                }
                println!("seq {last_seq}");
            }
            "sync" => {
                if words.len() != 1 {
                    return fail("bad sync", line);
                }
                if !kv.is_open() {
                    println!("reject sync reason=closed");
                    continue;
                }
                if let Err(error) = kv.sync() {
                    return probe_error(&error);
                }
                println!("sync");
            }
            "close" => {
                if words.len() != 1 {
                    return fail("bad close", line);
                }
                if let Err(error) = kv.close() {
                    return probe_error(&error);
                }
                println!("close ok=1");
            }
            "reopen" => {
                if words.len() != 1 {
                    return fail("bad reopen", line);
                }
                if let Err(error) = kv.init(false, 0) {
                    eprintln!("binlog_kv_probe: init failed: {error}");
                    return ExitCode::from(2);
                }
                println!("reopen ok=1 size={}", kv.size());
            }
            "dumpfile" => {
                if words.len() != 1 {
                    return fail("bad dumpfile", line);
                }
                if kv.is_open() {
                    // an open Binlog holds a whole-file write lock, so a second
                    // reader cannot see a single byte of it on Windows
                    println!("reject dumpfile reason=open");
                    continue;
                }
                let data = fs::read(&path).unwrap_or_default();
                println!("dumpfile size={} sha256={}", data.len(), sha256_hex(&data));
            }
            "raw" => {
                if words.len() != 3 {
                    return fail("bad raw", line);
                }
                let event_type: i32 = match words[1].parse() {
                    Ok(value) => value,
                    Err(_) => return fail("bad raw type", line),
                };
                let Some(data) = from_hex(words[2]) else {
                    return fail("bad raw hex", line);
                };
                if kv.is_open() {
                    println!("reject raw reason=open");
                    continue;
                }
                if data.len() % 4 != 0 {
                    println!("reject raw reason=unaligned");
                    continue;
                }
                let mut writer = match Writer::open(&path, false) {
                    Ok(writer) => writer,
                    Err(error) => return probe_error(&error),
                };
                let id = writer.next_event_id();
                let event = make_event(id, event_type, 0, &data);
                if let Err(error) = writer.add_frame(event) {
                    return probe_error(&error);
                }
                if let Err(error) = writer.sync() {
                    return probe_error(&error);
                }
                let ok = writer.close().is_ok();
                println!("raw type={event_type} id={id} datalen={} ok={}", data.len(), i32::from(ok));
            }
            "sq_set" => {
                if words.len() != 3 {
                    return fail("bad sq_set", line);
                }
                let (key, value) = match (from_hex(words[1]), from_hex(words[2])) {
                    (Some(key), Some(value)) => (key, value),
                    _ => return fail("bad sq_set hex", line),
                };
                if key.is_empty() {
                    println!("reject sq set key=- reason=empty-key");
                    continue;
                }
                let seq = memory.set(&key, &value);
                println!(
                    "sq set key={} value={} vlen={} seq={seq}",
                    to_hex(&key, key.len()),
                    to_hex(&value, HEX_PREVIEW),
                    value.len()
                );
            }
            "sq_erase" => {
                if words.len() != 2 {
                    return fail("bad sq_erase", line);
                }
                let Some(key) = from_hex(words[1]) else {
                    return fail("bad sq_erase hex", line);
                };
                let seq = memory.erase(&key);
                println!("sq erase key={} seq={seq}", to_hex(&key, key.len()));
            }
            "sq_erase_batch" => {
                if words.len() < 2 {
                    return fail("bad sq_erase_batch", line);
                }
                let mut keys = Vec::new();
                for word in &words[1..] {
                    match from_hex(word) {
                        Some(key) => keys.push(key),
                        None => return fail("bad sq_erase_batch hex", line),
                    }
                }
                let seq = memory.erase_batch(&keys);
                println!("sq erasebatch keys={} seq={seq}", keys.len());
            }
            "sq_get" | "sq_isset" => {
                if words.len() != 2 {
                    return fail("bad sq key command", line);
                }
                let Some(key) = from_hex(words[1]) else {
                    return fail("bad sq key hex", line);
                };
                let key_text = to_hex(&key, key.len());
                if command == "sq_get" {
                    let value = memory.map.get(&key).cloned().unwrap_or_default();
                    println!(
                        "sq get key={key_text} value={} vlen={}",
                        to_hex(&value, HEX_PREVIEW),
                        value.len()
                    );
                } else {
                    println!("sq isset key={key_text} value={}", i32::from(memory.map.contains_key(&key)));
                }
            }
            "sq_getall" => {
                if words.len() != 1 {
                    return fail("bad sq_getall", line);
                }
                let pairs: Vec<(Vec<u8>, Vec<u8>)> =
                    memory.map.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
                print!("sq getall n={}", pairs.len());
                print_pairs(&pairs);
                println!();
            }
            "sq_seq" => {
                if words.len() != 1 {
                    return fail("bad sq_seq", line);
                }
                println!("sq seq {}", memory.seq_no());
            }
            "sq_size" => {
                if words.len() != 1 {
                    return fail("bad sq_size", line);
                }
                println!("sq size {}", memory.map.len());
            }
            _ => return fail("unknown command", line),
        }
    }

    // td::Binlog::~Binlog closes (and with it syncs) the file, so the C++ probe
    // leaves every buffered frame on disk when it exits; mirror that.
    if let Err(error) = kv.close() {
        return probe_error(&error);
    }
    ExitCode::SUCCESS
}