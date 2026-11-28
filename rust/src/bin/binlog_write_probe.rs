//! Reproduce the td::Binlog write path, plaintext and AES-256-CTR encrypted.
//!
//! Frame (tddb/td/db/binlog/BinlogEvent.h:52-54), all little endian:
//!   [size:u32][id:u64][type:i32][flags:i32][extra:u64][data:size-32][crc32:u32]
//! `size` counts the whole frame including the trailing crc, `extra` is always 0
//! (BinlogEvent::create_raw stores a constant 0 there), and the crc is td::crc32
//! -- zlib's -- over the first `size - 4` bytes (BinlogEvent.cpp:66-70).
//!
//! Writer rules taken from Binlog.cpp / BinlogEvent.cpp:
//!   * `add`      -> frame(id = ++last_event_id, type, flags = 0, data)
//!   * `rewrite`  -> frame(id = *target*, type, flags = Rewrite (1), data); the
//!                   returned sequence number is burned in the id counter but is
//!                   never written, so it does not survive a reopen
//!   * `erase`    -> frame(id = target, type = Empty (-2), flags = 1, data empty)
//!   * `erase_batch` uses next_event_id(0) (returns last+1, counter unchanged)
//!                   and then burns one id per erased event
//!   * `add_event` LOG(FATAL)s when `size % 4 != 0` (Binlog.cpp:232), so a raw
//!                   payload length must be a multiple of 4; this side refuses
//!                   the same commands the oracle refuses
//!   * events are buffered (tier-1 `buffer_writer_`) and only `flush`/`sync`/
//!                   `close` push them to the file; `lazy_flush` flushes once the
//!                   buffer passes 1 << 14 bytes. Flush boundaries never change
//!                   the bytes, only how much of them is on disk at a given moment
//!   * `last_event_id` after a reopen is rebuilt by replay alone, and the replay
//!                   only counts *appended* events (BinlogEventsProcessor.cpp:17-58):
//!                   a rewrite keeps the target id, an erase drops it, and both
//!                   burn sequences that are lost -- so the counter can go
//!                   backwards across a reopen. A `Rewrite` frame whose target is
//!                   bigger than everything seen so far is appended as a live
//!                   event *with the flag still set*, while a service frame
//!                   (negative type) is skipped without touching the counter.
//!                   Writing through the processor does not move the counter
//!                   itself: `Binlog::last_event_id_` is a separate member.
//!   * the file is NOT append-only: `Binlog::add_event` reindexes once the file
//!                   passes 50 KiB with a 5:1 dead ratio (100 KiB at 4:1, 300 KiB
//!                   at 3:1, 500 KiB at 2:1), and `do_reindex` rewrites `path` as
//!                   the live set only -- each surviving frame keeps its original
//!                   raw bytes, so a reindex of a rewrite still says flags = 1.
//!                   The counters that trigger it are `fd_size_` (all bytes of
//!                   the current generation, buffered ones included) and
//!                   `total_raw_events_size_` (live frames only).
//!
//! Encryption -- `--key raw` (`DbKey::raw_key("cucumber")`) or a script
//! `change_key raw <hex>`:
//!   * a fresh file opened with a key is reindexed by `Binlog::init`, so the file
//!                   starts with one plaintext header event: id 0, type -3,
//!                   flags 0, data `[flags:u32 = 0][TL string key_salt (32B)]
//!                   [TL string iv (16B)][TL string key_hash (32B)]`, 96 bytes
//!                   (frame 128). `key = PBKDF2-HMAC-SHA256(db_key, key_salt, 2,
//!                   32)` for a raw key and `key_hash = HMAC-SHA256(key,
//!                   "cucumbers everywhere")`, both from tl_helpers'
//!                   `BEGIN/END_STORE_FLAGS` + `store(string)`.
//!   * everything after the header is AES-256-CTR ciphertext: the counter starts
//!                   at `iv` (128-bit big endian) and runs on across frame *and*
//!                   flush boundaries, because `AesCtrByteFlow::loop` encrypts
//!                   whatever is ready and advances the output by the same size
//!                   (partial blocks included).
//!   * `change_key` and every reindex regenerate the file: a new header with a
//!                   fresh salt and iv, then the live frames re-emitted and
//!                   re-encrypted from keystream offset 0.
//!   * opening an encrypted file with `DbKey::empty()` is `WrongPassword`
//!                   (`Binlog::init` fails before reaching the reindex branch), and
//!                   a wrong key fails the `key_hash` check the same way.
//!   * salt and iv are random per run (`Random::secure_bytes` in TDLib,
//!                   `BCryptGenRandom` here), so two encrypted files are never
//!                   byte equal. This side is judged by td's own decoder reading
//!                   the ciphertext it wrote, not by comparing bytes.
//!
//! Narration is identical to tools/fixtures/binlog_write_probe.cpp and the probe
//! must leave the same *plaintext* bytes behind; in plaintext mode that means the
//! very same file.
//!
//! Usage: binlog_write_probe [--key empty|raw] <script.txt> <out_binlog>

use std::env;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::process::ExitCode;

use aes::Aes256;
use ctr::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

#[allow(dead_code)]
#[path = "../binlog_format.rs"]
mod binlog_format;

use binlog_format::{
    create_raw, from_hex, store_tl_string, take_tl_string, u32_at, u64_at, DEFAULT_KEY, FLAG_REWRITE,
    HEADER_SIZE, KEY_CHECK, LAZY_FLUSH_THRESHOLD, MIN_SIZE, REINDEX_LEVELS, SERVICE_EMPTY,
    TAIL_SIZE,
};

const SERVICE_AES_CTR_ENCRYPTION: i32 = -3;
const DATA_PREVIEW: usize = 32;
/// `detail::AesCtrEncryptionEvent` for a raw key (`kdf_fast_iteration_count`)
const KDF_ITERATIONS: u32 = 2;
const SALT_SIZE: usize = 32;
const IV_SIZE: usize = 16;
const KEY_HASH_SIZE: usize = 32;
/// `BEGIN_STORE_FLAGS`/`END_STORE_FLAGS` write one zeroed `uint32` first
const HEADER_FLAGS_SIZE: usize = 4;
const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;

type Aes256Ctr = ctr::Ctr128BE<Aes256>;

#[link(name = "bcrypt")]
extern "system" {
    /// The RNG `Random::secure_bytes` ends up in on Windows.
    fn BCryptGenRandom(algorithm: *mut core::ffi::c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
}

fn secure_bytes(len: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; len];
    let status = unsafe {
        BCryptGenRandom(core::ptr::null_mut(), out.as_mut_ptr(), len as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
    };
    if status != 0 {
        return Err(format!("BCryptGenRandom failed: 0x{status:08x}"));
    }
    Ok(out)
}

/// `PBKDF2-HMAC-SHA256(db_key, key_salt, kdf_fast_iteration_count, 32)`.
fn derive_key(key: &[u8], salt: &[u8]) -> [u8; 32] {
    let mut derived = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(key, salt, KDF_ITERATIONS, &mut derived);
    derived
}

/// `AesCtrEncryptionEvent::generate_hash` (HMAC-SHA256 under a literal message).
fn key_hash(key: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256>>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(KEY_CHECK);
    mac.finalize().into_bytes().into()
}

/// XOR `data` with the AES-256-CTR keystream at byte position `offset`; CTR is
/// its own inverse, so this both encrypts and decrypts.
fn apply_keystream(data: &mut [u8], key: &[u8; 32], iv: &[u8], offset: usize) -> Result<(), String> {
    let mut iv_bytes = [0u8; IV_SIZE];
    if iv.len() != IV_SIZE {
        return Err(format!("iv must be {IV_SIZE} bytes, got {}", iv.len()));
    }
    iv_bytes.copy_from_slice(iv);
    let mut cipher = Aes256Ctr::new(key.into(), (&iv_bytes).into());
    cipher.seek(offset as u64);
    cipher.apply_keystream(data);
    Ok(())
}

/// The plaintext `type = -3` header event: three TL strings behind a flags word.
struct Header {
    salt: Vec<u8>,
    iv: Vec<u8>,
    hash: Vec<u8>,
}

impl Header {
    /// Fresh salt and iv, hash derived from the key -- `reset_encryption`.
    fn generate(key: &[u8]) -> Result<(Header, [u8; 32]), String> {
        let salt = secure_bytes(SALT_SIZE)?;
        let iv = secure_bytes(IV_SIZE)?;
        let derived = derive_key(key, &salt);
        let hash = key_hash(&derived).to_vec();
        Ok((Header { salt, iv, hash }, derived))
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_FLAGS_SIZE + SALT_SIZE + IV_SIZE + KEY_HASH_SIZE + 12);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&store_tl_string(&self.salt));
        out.extend_from_slice(&store_tl_string(&self.iv));
        out.extend_from_slice(&store_tl_string(&self.hash));
        out
    }

    fn parse(data: &[u8]) -> Option<Header> {
        if data.len() < HEADER_FLAGS_SIZE {
            return None;
        }
        let mut rest = &data[HEADER_FLAGS_SIZE..];
        let salt = take_tl_string(&mut rest)?;
        let iv = take_tl_string(&mut rest)?;
        let hash = take_tl_string(&mut rest)?;
        Some(Header {
            salt: salt.to_vec(),
            iv: iv.to_vec(),
            hash: hash.to_vec(),
        })
    }
}

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
        out.push('-');
    }
    out
}

/// A frame plus the parsed fields, exactly as `BinlogEvent` holds them.
#[derive(Clone)]
struct LogEvent {
    id: u64,
    event_type: i32,
    flags: i32,
    data: Vec<u8>,
    raw: Vec<u8>,
}

fn make_event(id: u64, event_type: i32, flags: i32, data: &[u8]) -> LogEvent {
    LogEvent {
        id,
        event_type,
        flags,
        data: data.to_vec(),
        raw: create_raw(id, event_type, flags, data),
    }
}

/// Frame walk of a plaintext binlog; stops at the first incomplete frame.
fn walk_frames(bytes: &[u8]) -> (Vec<LogEvent>, usize) {
    let mut events = Vec::new();
    let mut offset = 0;
    while offset + 4 <= bytes.len() {
        let size = u32_at(bytes, offset) as usize;
        if size < MIN_SIZE || offset + size > bytes.len() {
            break;
        }
        events.push(LogEvent {
            id: u64_at(bytes, offset + 4),
            event_type: u32_at(bytes, offset + 12) as i32,
            flags: u32_at(bytes, offset + 16) as i32,
            data: bytes[offset + HEADER_SIZE..offset + size - TAIL_SIZE].to_vec(),
            raw: bytes[offset..offset + size].to_vec(),
        });
        offset += size;
    }
    (events, offset)
}

struct Entry {
    /// `event_id * 2 + was_deleted`, the processor's ordering key
    fixed: u64,
    event: LogEvent,
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
    fn add(&mut self, event: LogEvent) -> Result<(), String> {
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

    fn live(&self) -> impl Iterator<Item = &LogEvent> {
        self.entries.iter().filter(|entry| !entry.deleted).map(|entry| &entry.event)
    }
}

fn load(bytes: &[u8]) -> Result<Processor, String> {
    let (frames, _) = walk_frames(bytes);
    let mut processor = Processor::default();
    for event in frames {
        processor.add(event)?;
    }
    Ok(processor)
}

struct Writer {
    path: String,
    file: Option<File>,
    buffer: Vec<u8>,
    processor: Processor,
    /// `Binlog::last_event_id_`, which only a reopen resynchronises from the
    /// processor -- writing through the processor does not move this counter, so
    /// the rewrite-of-a-future-id quirk cannot drag the sequence number forward
    last_event_id: u64,
    /// all bytes of the current file generation, buffered ones included
    fd_size: usize,
    need_sync: bool,
    /// `DbKey` in use; `None` is `DbKey::empty()` and means plaintext
    key: Option<Vec<u8>>,
    header: Option<Header>,
    /// PBKDF2 of `key` under `header.salt`, cached for the whole generation
    derived: Option<[u8; 32]>,
    /// ciphertext bytes written since the header, i.e. the keystream position
    cipher_offset: usize,
}

impl Writer {
    fn open(path: &str, key: Option<Vec<u8>>) -> Result<Self, String> {
        let _ = fs::remove_file(format!("{path}.new"));
        let _ = fs::remove_file(path);
        let mut writer = Writer {
            path: path.to_owned(),
            file: None,
            buffer: Vec::new(),
            processor: Processor::default(),
            last_event_id: 0,
            fd_size: 0,
            need_sync: false,
            key,
            header: None,
            derived: None,
            cipher_offset: 0,
        };
        writer.reopen()?;
        Ok(writer)
    }

    fn next_event_id(&mut self) -> u64 {
        self.last_event_id += 1;
        self.last_event_id
    }

    fn reopen(&mut self) -> Result<(), String> {
        if self.file.is_some() {
            self.close()?;
        }
        // `Binlog::init` opens the file before it loads it, so the reindex that
        // installs encryption on a fresh file has somewhere to write.
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.path)
            .map_err(|error| format!("open {}: {error}", self.path))?;
        self.file = Some(file);
        if let Err(error) = self.load_file() {
            // `Binlog::init` closes the binlog before reporting WrongPassword
            self.file = None;
            self.header = None;
            self.derived = None;
            return Err(error);
        }
        self.file
            .as_mut()
            .ok_or_else(|| "reopen lost the fd".to_owned())?
            .seek(SeekFrom::End(0))
            .map_err(|error| format!("seek: {error}"))?;
        Ok(())
    }

    /// Read the file, verify or install the header, replay it and resync.
    fn load_file(&mut self) -> Result<(), String> {
        let bytes = fs::read(&self.path).unwrap_or_default();
        let (raw_frames, _) = walk_frames(&bytes);
        let mut header = None;
        let mut body_start = 0;
        if let Some(first) = raw_frames.first() {
            if first.id == 0 && first.event_type == SERVICE_AES_CTR_ENCRYPTION {
                header = Some(Header::parse(&first.data).ok_or_else(|| "bad encryption header".to_owned())?);
                body_start = first.raw.len();
            }
        }
        let body = &bytes[body_start..];
        let mut generate = false;
        match (&self.key, &header) {
            (Some(key), Some(found)) => {
                let derived = derive_key(key, &found.salt);
                if key_hash(&derived)[..] != found.hash[..] {
                    return Err("Wrong password".to_owned());
                }
                self.derived = Some(derived);
            }
            (Some(_), None) => generate = true,
            // `Binlog::init` reports WrongPassword before it can reindex
            (None, Some(_)) => return Err("Wrong password".to_owned()),
            (None, None) => {}
        }
        let plain = match (&self.derived, &header) {
            (Some(derived), Some(found)) => {
                let mut plain = body.to_vec();
                apply_keystream(&mut plain, derived, &found.iv, 0)?;
                plain
            }
            _ => body.to_vec(),
        };
        let processor = load(&plain)?;
        for event in processor.live() {
            println!(
                "replay id={} type={} flags={} datalen={} data={}",
                event.id,
                event.event_type,
                event.flags,
                event.data.len(),
                to_hex(&event.data, DATA_PREVIEW)
            );
        }
        self.buffer.clear();
        self.last_event_id = processor.last_event_id;
        self.processor = processor;
        self.header = header;
        self.fd_size = bytes.len();
        self.cipher_offset = if self.header.is_some() { bytes.len() - body_start } else { 0 };
        if generate {
            self.write_generation()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let mut chunk = std::mem::take(&mut self.buffer);
        if let (Some(derived), Some(header)) = (&self.derived, &self.header) {
            apply_keystream(&mut chunk, derived, &header.iv, self.cipher_offset)?;
            self.cipher_offset += chunk.len();
        }
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
    fn add_frame(&mut self, event: LogEvent) -> Result<(), String> {
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

    /// `Binlog::change_key`: swap the key and regenerate the file.
    fn change_key(&mut self, key: Option<Vec<u8>>) -> Result<(), String> {
        self.key = key;
        self.write_generation()
    }

    /// `Binlog::do_reindex`: flush, then regenerate `path` from the live set with
    /// a fresh header when a key is in use (this is also the path that installs
    /// encryption on a plaintext file and drops it again).
    fn write_generation(&mut self) -> Result<(), String> {
        self.flush()?;
        let mut out = Vec::new();
        let mut header = None;
        let mut derived = None;
        if let Some(key) = &self.key {
            let (fresh, fresh_key) = Header::generate(key)?;
            out.extend_from_slice(&create_raw(0, SERVICE_AES_CTR_ENCRYPTION, 0, &fresh.encode()));
            header = Some(fresh);
            derived = Some(fresh_key);
        }
        let mut body = Vec::with_capacity(self.processor.total_raw);
        for event in self.processor.live() {
            body.extend_from_slice(&event.raw);
        }
        if let (Some(fresh_key), Some(fresh)) = (&derived, &header) {
            apply_keystream(&mut body, fresh_key, &fresh.iv, 0)?;
        }
        let cipher_offset = body.len();
        out.extend_from_slice(&body);
        let file = self.file.as_mut().ok_or_else(|| "reindex without an open binlog".to_owned())?;
        file.set_len(0).map_err(|error| format!("truncate: {error}"))?;
        file.seek(SeekFrom::Start(0)).map_err(|error| format!("seek: {error}"))?;
        file.write_all(&out).map_err(|error| format!("write: {error}"))?;
        self.need_sync = true;
        self.fd_size = out.len();
        self.cipher_offset = if header.is_some() { cipher_offset } else { 0 };
        self.header = header;
        self.derived = derived;
        Ok(())
    }
}

fn print_summary(path: &str) -> Result<(), String> {
    let bytes = fs::read(path).unwrap_or_default();
    // An encrypted binlog ends where its first frame ends: everything after that
    // is ciphertext under a random iv, so a frame walk and a hash are meaningless.
    if bytes.len() >= MIN_SIZE {
        let size = u32_at(&bytes, 0) as usize;
        if (MIN_SIZE..=bytes.len()).contains(&size) && u32_at(&bytes, 12) as i32 == SERVICE_AES_CTR_ENCRYPTION {
            println!("summary encrypted size={}", bytes.len());
            return Ok(());
        }
    }
    let (frames, consumed) = walk_frames(&bytes);
    let digest = Sha256::digest(&bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    println!(
        "summary frames={} size={} sha256={} leftover={}",
        frames.len(),
        bytes.len(),
        hex,
        bytes.len() - consumed
    );
    Ok(())
}

fn fail(what: &str, line: &str) -> ExitCode {
    eprintln!("binlog_write_probe: {what} in line {line}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let mut index = 1;
    let mut key: Option<Vec<u8>> = None;
    if args.len() == 5 {
        if args[1] != "--key" || (args[2] != "empty" && args[2] != "raw") {
            eprintln!("Usage: binlog_write_probe [--key empty|raw] <script.txt> <out_binlog>");
            return ExitCode::from(2);
        }
        if args[2] == "raw" {
            key = Some(DEFAULT_KEY.to_vec());
        }
        index = 3;
    }
    if args.len() - index != 2 {
        eprintln!("Usage: binlog_write_probe [--key empty|raw] <script.txt> <out_binlog>");
        return ExitCode::from(2);
    }
    let script_path = &args[index];
    let out_path = &args[index + 1];

    let mut writer = match Writer::open(out_path, key) {
        Ok(writer) => writer,
        Err(error) => {
            eprintln!("binlog_write_probe: init failed: {error}");
            return ExitCode::from(2);
        }
    };
    let script = match fs::read_to_string(script_path) {
        Ok(script) => script,
        Err(error) => {
            eprintln!("binlog_write_probe: cannot read {script_path}: {error}");
            return ExitCode::from(2);
        }
    };

    for raw_line in script.lines() {
        let line = raw_line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let command = words[0];

        match command {
            "add" | "addtl" => {
                if words.len() != 3 {
                    return fail("bad add", line);
                }
                let event_type: i32 = match words[1].parse() {
                    Ok(value) => value,
                    Err(_) => return fail("bad type", line),
                };
                let payload = match from_hex(words[2]) {
                    Some(payload) => payload,
                    None => return fail("bad hex", line),
                };
                let event = if command == "add" {
                    if payload.len() % 4 != 0 {
                        println!("skip add type={event_type} len={} reason=unaligned", payload.len());
                        continue;
                    }
                    let id = writer.next_event_id();
                    println!("add type={event_type} id={id} datalen={}", payload.len());
                    make_event(id, event_type, 0, &payload)
                } else {
                    let data = store_tl_string(&payload);
                    let id = writer.next_event_id();
                    println!(
                        "addtl type={event_type} id={id} datalen={} payload={}",
                        data.len(),
                        payload.len()
                    );
                    make_event(id, event_type, 0, &data)
                };
                if let Err(error) = writer.add_frame(event) {
                    eprintln!("binlog_write_probe: {error}");
                    return ExitCode::from(2);
                }
            }
            "rewrite" => {
                if words.len() != 4 {
                    return fail("bad rewrite", line);
                }
                let target: u64 = match words[1].parse() {
                    Ok(value) => value,
                    Err(_) => return fail("bad target", line),
                };
                let event_type: i32 = match words[2].parse() {
                    Ok(value) => value,
                    Err(_) => return fail("bad type", line),
                };
                let payload = match from_hex(words[3]) {
                    Some(payload) => payload,
                    None => return fail("bad hex", line),
                };
                if payload.len() % 4 != 0 {
                    println!(
                        "skip rewrite target={target} type={event_type} len={} reason=unaligned",
                        payload.len()
                    );
                    continue;
                }
                let seq = writer.next_event_id();
                println!(
                    "rewrite seq={seq} target={target} type={event_type} datalen={}",
                    payload.len()
                );
                let event = make_event(target, event_type, FLAG_REWRITE, &payload);
                if let Err(error) = writer.add_frame(event) {
                    eprintln!("binlog_write_probe: {error}");
                    return ExitCode::from(2);
                }
            }
            "erase" => {
                if words.len() != 2 {
                    return fail("bad erase", line);
                }
                let target: u64 = match words[1].parse() {
                    Ok(value) => value,
                    Err(_) => return fail("bad target", line),
                };
                let seq = writer.next_event_id();
                println!("erase seq={seq} target={target}");
                let event = make_event(target, SERVICE_EMPTY, FLAG_REWRITE, &[]);
                if let Err(error) = writer.add_frame(event) {
                    eprintln!("binlog_write_probe: {error}");
                    return ExitCode::from(2);
                }
            }
            "erasebatch" => {
                if words.len() != 2 {
                    return fail("bad erasebatch", line);
                }
                let targets: Vec<u64> = match words[1]
                    .split(',')
                    .map(|item| item.parse::<u64>())
                    .collect::<Result<_, _>>()
                {
                    Ok(targets) => targets,
                    Err(_) => return fail("bad target list", line),
                };
                let seq = writer.last_event_id + 1;
                for &target in &targets {
                    writer.next_event_id();
                    let event = make_event(target, SERVICE_EMPTY, FLAG_REWRITE, &[]);
                    if let Err(error) = writer.add_frame(event) {
                        eprintln!("binlog_write_probe: {error}");
                        return ExitCode::from(2);
                    }
                }
                println!("erasebatch seq={seq} targets={}", targets.len());
            }
            "change_key" => {
                if words.len() < 2 {
                    return fail("bad change_key", line);
                }
                let key = match words[1] {
                    "empty" => None,
                    "raw" => {
                        if words.len() != 3 {
                            return fail("bad change_key", line);
                        }
                        match from_hex(words[2]) {
                            Some(key) => Some(key),
                            None => return fail("bad change_key hex", line),
                        }
                    }
                    _ => return fail("bad change_key mode", line),
                };
                // `Binlog::change_key` -> do_reindex: a fresh header (new salt and
                // iv) and the live frames re-encrypted under the new key.
                if let Err(error) = writer.change_key(key) {
                    eprintln!("binlog_write_probe: {error}");
                    return ExitCode::from(2);
                }
                println!("change_key {}", words[1]);
            }
            "lazy" => {
                if let Err(error) = writer.lazy_flush() {
                    eprintln!("binlog_write_probe: {error}");
                    return ExitCode::from(2);
                }
                println!("lazy");
            }
            "flush" => {
                if let Err(error) = writer.flush() {
                    eprintln!("binlog_write_probe: {error}");
                    return ExitCode::from(2);
                }
                println!("flush");
            }
            "sync" => {
                if let Err(error) = writer.sync() {
                    eprintln!("binlog_write_probe: {error}");
                    return ExitCode::from(2);
                }
                println!("sync");
            }
            "close" => {
                let ok = writer.close().is_ok();
                println!("close ok={}", i32::from(ok));
            }
            "reopen" => {
                let ok = writer.reopen().is_ok();
                println!("reopen ok={}", i32::from(ok));
            }
            "nextid" => {
                let id = writer.next_event_id();
                println!("nextid {id}");
            }
            _ => return fail("unknown command", line),
        }
    }

    if let Err(error) = print_summary(out_path) {
        eprintln!("binlog_write_probe: {error}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}