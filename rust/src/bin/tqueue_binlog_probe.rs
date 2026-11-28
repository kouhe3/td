//! Judge `td::TQueueBinlog<td::Binlog>` (tddb/td/db/TQueue.h) against the C++
//! oracle (`tools/fixtures/tqueue_binlog_probe.cpp`).
//!
//!   tqueue_binlog_probe [--key empty|raw] <script.txt> <binlog_path>
//!
//! The queue is the real `TQueueImpl` from `rust/src/tqueue.rs`; the storage
//! callback turns every mutation into a binlog frame, and `replay` reads those
//! frames back. The writer here is the minimal shape the queue needs
//! (`add`/`rewrite`/`erase`/`erase_batch` + the id counter). With `--key raw`
//! the file gets the same `type = -3` header and AES-256-CTR layer as
//! `binlog_write_probe` (shared `binlog_crypto`); reindex under a key stays
//! out of scope and is exercised by the binlog lanes.
//!
//! Frame body (`TQueueLogEvent::store`, TQueue.cpp):
//!   queue_id:i64 ‖ event_id:i32 ‖ expires_at:i32 ‖ data:TL-string ‖ [extra:i64]
//! The trailing `extra` is written **only when it is not zero**, which is also
//! what selects the magic: `BINLOG_EVENT_TYPE + (extra != 0)` = 2314 or 2315.
//! A rewrite re-reads the *current* extra, so an event whose data was cleared
//! and whose extra was 0 rewrites as a 2314 frame.
//!
//! `erase` writes `type = -2` (`SERVICE_EMPTY`), `flags = 1`, no data at all,
//! and its id is the erased event's log_event_id. `erase_batch` takes
//! `next_event_id(0)` first (the counter does not move) and then erases one id
//! at a time.

#[allow(dead_code)]
#[path = "../binlog_format.rs"]
mod binlog_format;
#[allow(dead_code)]
#[path = "../binlog_crypto.rs"]
mod binlog_crypto;
#[allow(dead_code)]
#[path = "../tqueue.rs"]
mod tqueue;

use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::rc::Rc;

use binlog_crypto::{apply_keystream, derive_key, key_hash, Header, SERVICE_AES_CTR_ENCRYPTION};
use binlog_format::{
    make_event, store_tl_string, take_tl_string, walk_frames, Frame, Processor, FLAG_REWRITE, SERVICE_EMPTY,
};
use tqueue::{EventId, RawEvent, StorageCallback, TQueueImpl};

const VALUE_CAP: usize = 32;
const BINLOG_EVENT_TYPE: i32 = 2314;
const QUEUE_EVENT_HEADER: usize = 8 + 4 + 4;

fn hex_all(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex_head(bytes: &[u8]) -> String {
    if bytes.len() <= VALUE_CAP {
        hex_all(bytes)
    } else {
        hex_all(&bytes[..VALUE_CAP])
    }
}

fn parse_i32(text: &str) -> Option<i32> {
    text.parse::<i32>().ok()
}

fn parse_i64(text: &str) -> Option<i64> {
    text.parse::<i64>().ok()
}

/// The minimum of `td::Binlog` that `TQueueBinlog` touches.
struct Binlog {
    file: Option<File>,
    buffer: Vec<u8>,
    last_event_id: u64,
    /// `DbKey` in use; `None` is `DbKey::empty()` and means plaintext
    key: Option<Vec<u8>>,
    header: Option<Header>,
    derived: Option<[u8; 32]>,
    /// ciphertext bytes written since the header, i.e. the keystream position
    cipher_offset: usize,
}

impl Binlog {
    /// `Binlog::init`: open the file, verify the key against its header (or
    /// write a fresh header when a key is in use), and hand back the plaintext
    /// frames the caller replays.
    fn open(path: &str, fresh: bool, key: Option<Vec<u8>>) -> Result<(Binlog, Vec<Frame>), String> {
        if fresh {
            let _ = std::fs::remove_file(path);
        }
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(fresh)
            .open(path)
            .map_err(|error| format!("open {path}: {error}"))?;
        let bytes = if fresh {
            Vec::new()
        } else {
            std::fs::read(path).map_err(|error| format!("read {path}: {error}"))?
        };
        let (header, derived, body_start, frames) = decode_file(&bytes, key.as_deref())?;
        let mut binlog = Binlog {
            file: Some(file),
            buffer: Vec::new(),
            last_event_id: 0,
            key,
            header,
            derived,
            cipher_offset: bytes.len() - body_start,
        };
        if binlog.header.is_none() {
            if let Some(key) = binlog.key.clone() {
                // `Binlog::init` reindexes a file opened with a key: the header
                // frame is plaintext and everything after it is keystream.
                let (header, derived) = Header::generate(&key)?;
                let raw = make_event(0, SERVICE_AES_CTR_ENCRYPTION, 0, &header.encode()).raw;
                let file = binlog.file.as_mut().ok_or_else(|| "binlog closed".to_owned())?;
                file.write_all(&raw).map_err(|error| format!("write: {error}"))?;
                file.sync_all().map_err(|error| format!("sync: {error}"))?;
                binlog.header = Some(header);
                binlog.derived = Some(derived);
                binlog.cipher_offset = 0;
            }
        }
        Ok((binlog, frames))
    }

    fn next_event_id(&mut self) -> u64 {
        self.last_event_id += 1;
        self.last_event_id
    }

    fn next_event_id_shift(&mut self, shift: i32) -> u64 {
        let result = self.last_event_id + 1;
        self.last_event_id = self.last_event_id.wrapping_add(shift as u64);
        result
    }

    fn add_raw(&mut self, event: Frame) {
        self.buffer.extend_from_slice(&event.raw);
    }

    fn add(&mut self, event_type: i32, data: &[u8]) -> u64 {
        let id = self.next_event_id();
        self.add_raw(make_event(id, event_type, 0, data));
        id
    }

    fn rewrite(&mut self, event_id: u64, event_type: i32, data: &[u8]) -> u64 {
        let seq_no = self.next_event_id();
        self.add_raw(make_event(event_id, event_type, FLAG_REWRITE, data));
        seq_no
    }

    fn erase(&mut self, event_id: u64) -> u64 {
        let seq_no = self.next_event_id();
        self.add_raw(make_event(event_id, SERVICE_EMPTY, FLAG_REWRITE, &[]));
        seq_no
    }

    fn erase_batch(&mut self, event_ids: Vec<u64>) -> u64 {
        if event_ids.is_empty() {
            return 0;
        }
        let seq_no = self.next_event_id_shift(0);
        for event_id in event_ids {
            self.erase(event_id);
        }
        seq_no
    }

    fn close(&mut self) -> Result<(), String> {
        if let Some(mut file) = self.file.take() {
            let mut chunk = std::mem::take(&mut self.buffer);
            if let (Some(derived), Some(header)) = (&self.derived, &self.header) {
                apply_keystream(&mut chunk, derived, &header.iv, self.cipher_offset)?;
                self.cipher_offset += chunk.len();
            }
            file.write_all(&chunk).map_err(|error| format!("write: {error}"))?;
            file.sync_all().map_err(|error| format!("sync: {error}"))?;
        }
        self.buffer.clear();
        Ok(())
    }
}

/// Strip and verify the `type = -3` header, undo the keystream, and walk the
/// plaintext body. Returns `(header, derived key, body offset, live frames)`.
fn decode_file(
    bytes: &[u8],
    key: Option<&[u8]>,
) -> Result<(Option<Header>, Option<[u8; 32]>, usize, Vec<Frame>), String> {
    let raw = walk_frames(bytes);
    let mut header = None;
    let mut body_start = 0;
    if let Some(first) = raw.first() {
        if first.id == 0 && first.event_type == SERVICE_AES_CTR_ENCRYPTION {
            header = Some(Header::parse(&first.data).ok_or_else(|| "bad encryption header".to_owned())?);
            body_start = first.raw.len();
        }
    }
    let mut plain = bytes[body_start..].to_vec();
    let mut derived = None;
    match (key, &header) {
        (Some(key), Some(found)) => {
            let value = derive_key(key, &found.salt);
            if key_hash(&value)[..] != found.hash[..] {
                return Err("Wrong password".to_owned());
            }
            apply_keystream(&mut plain, &value, &found.iv, 0)?;
            derived = Some(value);
        }
        // `Binlog::init` reports WrongPassword before it can reindex
        (None, Some(_)) => return Err("Wrong password".to_owned()),
        _ => {}
    }
    Ok((header, derived, body_start, walk_frames(&plain)))
}

/// `TQueueLogEvent::store`.
fn store_tqueue_event(queue_id: i64, event: &RawEvent) -> Vec<u8> {
    let mut out = Vec::with_capacity(QUEUE_EVENT_HEADER + event.data.len() + 12);
    out.extend_from_slice(&queue_id.to_le_bytes());
    out.extend_from_slice(&event.event_id.value().to_le_bytes());
    out.extend_from_slice(&event.expires_at.to_le_bytes());
    out.extend_from_slice(&store_tl_string(&event.data));
    if event.extra != 0 {
        out.extend_from_slice(&event.extra.to_le_bytes());
    }
    out
}

/// `TQueueLogEvent::parse` plus the `fetch_end` check.
fn parse_tqueue_event(data: &[u8], has_extra: i32) -> Result<(i64, i32, i32, Vec<u8>, i64), String> {
    if data.len() < QUEUE_EVENT_HEADER {
        return Err("Not enough data to read".to_string());
    }
    let queue_id = i64::from_le_bytes(data[0..8].try_into().unwrap());
    let event_id = i32::from_le_bytes(data[8..12].try_into().unwrap());
    let expires_at = i32::from_le_bytes(data[12..16].try_into().unwrap());
    let mut rest = &data[QUEUE_EVENT_HEADER..];
    let payload = take_tl_string(&mut rest).ok_or_else(|| "Not enough data to read".to_string())?;
    let payload = payload.to_vec();
    let extra = if has_extra == 1 {
        if rest.len() < 8 {
            return Err("Not enough data to read".to_string());
        }
        let value = i64::from_le_bytes(rest[0..8].try_into().unwrap());
        rest = &rest[8..];
        value
    } else {
        0
    };
    if !rest.is_empty() {
        return Err("Too much data to fetch".to_string());
    }
    Ok((queue_id, event_id, expires_at, payload, extra))
}

/// `TQueueBinlog<Binlog>::replay`.
fn replay_event(event: &Frame, queue: &mut TQueueImpl) -> Result<(), String> {
    let has_extra = event.event_type - BINLOG_EVENT_TYPE;
    if has_extra != 0 && has_extra != 1 {
        return Err("Wrong magic".to_string());
    }
    let (queue_id, event_id, expires_at, data, extra) = parse_tqueue_event(&event.data, has_extra)?;
    let event_id = EventId::from_int32(event_id)?;
    let raw = RawEvent {
        log_event_id: event.id,
        event_id,
        expires_at,
        data,
        extra,
    };
    if !queue.do_push(queue_id, raw) {
        return Err("Failed to add event".to_string());
    }
    Ok(())
}

/// `TQueueBinlog<Binlog>` as the queue's `StorageCallback`.
struct TQueueBinlogStorage(Rc<RefCell<Binlog>>);

impl StorageCallback for TQueueBinlogStorage {
    fn push(&mut self, queue_id: i64, event: &RawEvent) -> u64 {
        let payload = store_tqueue_event(queue_id, event);
        let magic = BINLOG_EVENT_TYPE + i32::from(event.extra != 0);
        let mut binlog = self.0.borrow_mut();
        if event.log_event_id == 0 {
            binlog.add(magic, &payload)
        } else {
            binlog.rewrite(event.log_event_id, magic, &payload);
            event.log_event_id
        }
    }

    fn pop(&mut self, log_event_id: u64) {
        self.0.borrow_mut().erase(log_event_id);
    }

    fn pop_batch(&mut self, log_event_ids: Vec<u64>) {
        self.0.borrow_mut().erase_batch(log_event_ids);
    }

    fn close(&mut self) {
        self.0.borrow_mut().close().ok();
    }
}

fn print_events(events: &[tqueue::Event]) {
    for event in events {
        print!(
            " id={} exp={} extra={} data={} vlen={}",
            event.id.value(),
            event.expires_at,
            event.extra,
            hex_head(&event.data),
            event.data.len()
        );
    }
}

struct Env {
    path: String,
    /// `DbKey` in use; `None` is `DbKey::empty()` and means plaintext
    key: Option<Vec<u8>>,
    binlog: Option<Rc<RefCell<Binlog>>>,
    queue: Option<TQueueImpl>,
    open: bool,
}

impl Env {
    fn init(&mut self, fresh: bool, replayed: Option<&mut u64>) -> Result<(), String> {
        let (binlog, frames) = Binlog::open(&self.path, fresh, self.key.clone())?;
        let mut queue = TQueueImpl::create();
        let mut seen = 0u64;
        // A fresh file has no frames; an existing one is replayed before the
        // callback is installed, exactly like the oracle's `helper` instance.
        // `Binlog::init` hands over the processor's *live* set, not the raw
        // frames, so folded rewrites and erased ids never reach the queue.
        let mut processor = Processor::default();
        for frame in frames {
            if let Err(error) = processor.add(frame) {
                println!("replay err={error}");
            }
        }
        for frame in processor.live() {
            match replay_event(frame, &mut queue) {
                Ok(()) => seen += 1,
                Err(error) => println!("replay err={error}"),
            }
        }
        let binlog = Rc::new(RefCell::new(binlog));
        queue.set_callback(Box::new(TQueueBinlogStorage(Rc::clone(&binlog))));
        self.binlog = Some(binlog);
        self.queue = Some(queue);
        self.open = true;
        if let Some(slot) = replayed {
            *slot = seen;
        }
        Ok(())
    }

    fn close(&mut self) -> bool {
        if !self.open {
            return true;
        }
        self.open = false;
        match self.binlog.as_ref() {
            Some(binlog) => match binlog.borrow_mut().close() {
                Ok(()) => true,
                Err(error) => {
                    println!("close err={error}");
                    false
                }
            },
            None => true,
        }
    }
}

fn run_script(script_path: &str, binlog_path: &str, key: Option<Vec<u8>>) -> i32 {
    let file = match File::open(script_path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("cannot read {script_path}: {e}");
            return 2;
        }
    };
    use std::io::BufRead;

    let script_lines: Vec<String> = std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .collect();
    let attach = script_lines
        .iter()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .find(|tokens| !tokens.is_empty() && !tokens[0].starts_with('#'))
        .is_some_and(|tokens| tokens[0] == "attach");

    if !attach {
        let _ = std::fs::remove_file(binlog_path);
    }
    let mut env = Env {
        path: binlog_path.to_string(),
        key,
        binlog: None,
        queue: None,
        open: false,
    };
    let mut seen = 0u64;
    if let Err(error) = env.init(!attach, Some(&mut seen)) {
        println!("init err={error}");
        return 3;
    }
    if attach {
        println!("attach frames={seen}");
    }
    let mut result_events: Vec<tqueue::Event> = Vec::new();

    for line in &script_lines {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() || tokens[0].starts_with('#') {
            continue;
        }
        let command = tokens[0];

        match command {
            "push" => {
                if tokens.len() != 6 {
                    println!("reject push argc={}", tokens.len());
                    continue;
                }
                let (Some(queue_id), Some(data), Some(expires_at), Some(extra)) = (
                    parse_i64(tokens[1]),
                    binlog_format::from_hex(tokens[2]),
                    parse_i32(tokens[3]),
                    parse_i64(tokens[4]),
                ) else {
                    println!("reject push argc={}", tokens.len());
                    continue;
                };
                let mut hint = EventId::default();
                if tokens[5] != "-" {
                    let Some(hint_value) = parse_i32(tokens[5]) else {
                        println!("reject push reason=hint-not-int32");
                        continue;
                    };
                    match EventId::from_int32(hint_value) {
                        Ok(id) => hint = id,
                        Err(e) => {
                            println!("push err={e}");
                            continue;
                        }
                    }
                }
                match env.queue.as_mut().unwrap().push(queue_id, data.clone(), expires_at, extra, hint) {
                    Ok(id) => println!(
                        "push q={queue_id} id={} data={} vlen={}",
                        id.value(),
                        hex_head(&data),
                        data.len()
                    ),
                    Err(e) => println!("push err={e}"),
                }
            }
            "get" => {
                if tokens.len() != 6 {
                    println!("reject get argc={}", tokens.len());
                    continue;
                }
                let (Some(queue_id), Some(forget), Some(now), Some(max_n)) = (
                    parse_i64(tokens[1]),
                    parse_i32(tokens[3]),
                    parse_i32(tokens[4]),
                    parse_i32(tokens[5]),
                ) else {
                    println!("reject get argc={}", tokens.len());
                    continue;
                };
                if max_n < 0 {
                    println!("reject get argc={}", tokens.len());
                    continue;
                }
                let mut from_id = EventId::default();
                if tokens[2] != "-" {
                    let Some(from_value) = parse_i32(tokens[2]) else {
                        println!("reject get reason=from-not-int32");
                        continue;
                    };
                    match EventId::from_int32(from_value) {
                        Ok(id) => from_id = id,
                        Err(e) => {
                            println!("get err={e}");
                            continue;
                        }
                    }
                }
                let outcome = env.queue.as_mut().unwrap().get(
                    queue_id,
                    from_id,
                    forget != 0,
                    now,
                    max_n as usize,
                    &mut result_events,
                );
                match outcome {
                    Ok(size) => {
                        print!("get q={queue_id} ret={size} n={}", result_events.len());
                        print_events(&result_events);
                        println!();
                    }
                    Err(e) => println!("get err={e}"),
                }
            }
            "forget" => {
                let (Some(queue_id), Some(id_value)) =
                    (tokens.get(1).and_then(|t| parse_i64(t)), tokens.get(2).and_then(|t| parse_i32(t)))
                else {
                    println!("reject forget argc={}", tokens.len());
                    continue;
                };
                match EventId::from_int32(id_value) {
                    Ok(id) => {
                        env.queue.as_mut().unwrap().forget(queue_id, id);
                        println!("forget q={queue_id} id={id_value}");
                    }
                    Err(e) => println!("forget err={e}"),
                }
            }
            "clear" => {
                let (Some(queue_id), Some(keep)) =
                    (tokens.get(1).and_then(|t| parse_i64(t)), tokens.get(2).and_then(|t| parse_i64(t)))
                else {
                    println!("reject clear argc={}", tokens.len());
                    continue;
                };
                if keep < 0 {
                    println!("reject clear argc={}", tokens.len());
                    continue;
                }
                let queue = env.queue.as_mut().unwrap();
                let deleted = queue.clear(queue_id, keep as usize);
                let ids: Vec<String> = deleted.keys().map(|id| id.value().to_string()).collect();
                println!(
                    "clear q={queue_id} keep={keep} returned={} del={} head={} tail={} size={}",
                    deleted.len(),
                    if ids.is_empty() { "-".to_string() } else { ids.join(",") },
                    queue.get_head(queue_id).value(),
                    queue.get_tail(queue_id).value(),
                    queue.get_size(queue_id)
                );
            }
            "head" | "tail" | "size" => {
                let Some(queue_id) = tokens.get(1).and_then(|t| parse_i64(t)) else {
                    println!("reject {command} argc={}", tokens.len());
                    continue;
                };
                let queue = env.queue.as_ref().unwrap();
                match command {
                    "head" => println!("head q={queue_id} id={}", queue.get_head(queue_id).value()),
                    "tail" => println!("tail q={queue_id} id={}", queue.get_tail(queue_id).value()),
                    _ => println!("size q={queue_id} n={}", queue.get_size(queue_id)),
                }
            }
            "gc" => {
                let Some(now) = tokens.get(1).and_then(|t| parse_i32(t)) else {
                    println!("reject gc argc={}", tokens.len());
                    continue;
                };
                let (deleted, completed) = env.queue.as_mut().unwrap().run_gc(now);
                println!("gc now={now} deleted={deleted} completed={}", i32::from(completed));
            }
            "close" => {
                let ok = env.close();
                println!("close ok={}", i32::from(ok));
            }
            "dumpfile" => {
                if env.open {
                    println!("reject dumpfile reason=open");
                    continue;
                }
                match std::fs::read(binlog_path) {
                    Ok(bytes) => {
                        use sha2::{Digest, Sha256};
                        let mut hasher = Sha256::new();
                        hasher.update(&bytes);
                        println!("dumpfile size={} sha256={}", bytes.len(), hex_all(&hasher.finalize()));
                    }
                    Err(_) => println!("reject dumpfile reason=missing"),
                }
            }
            "replay" => {
                if env.open {
                    println!("reject replay reason=open");
                    continue;
                }
                if tokens.len() < 2 {
                    println!("reject replay argc={}", tokens.len());
                    continue;
                }
                let bytes = std::fs::read(binlog_path).unwrap_or_default();
                let frames = match decode_file(&bytes, env.key.as_deref()) {
                    Ok((_, _, _, frames)) => frames,
                    Err(error) => {
                        println!("replay err={error}");
                        continue;
                    }
                };
                let mut fresh = TQueueImpl::create();
                let mut seen = 0u64;
                let mut processor = Processor::default();
                for frame in frames {
                    if let Err(error) = processor.add(frame) {
                        println!("replay err={error}");
                    }
                }
                for frame in processor.live() {
                    match replay_event(frame, &mut fresh) {
                        Ok(()) => seen += 1,
                        Err(error) => println!("replay err={error}"),
                    }
                }
                for token in &tokens[1..] {
                    let Some(queue_id) = parse_i64(token) else {
                        println!("reject replay queue={token}");
                        continue;
                    };
                    println!(
                        "replay q={queue_id} head={} tail={} size={}",
                        fresh.get_head(queue_id).value(),
                        fresh.get_tail(queue_id).value(),
                        fresh.get_size(queue_id)
                    );
                }
                println!("replay frames={seen}");
            }
            "attach" => continue,
            other => println!("reject unknown={other}"),
        }
    }
    if env.open {
        env.close();
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: tqueue_binlog_probe [--key empty|raw] <script.txt> <binlog_path>";
    let mut index = 1;
    let mut key = None;
    if args.len() > 1 && args[1] == "--key" {
        match args.get(2).map(String::as_str) {
            Some("empty") => {}
            Some("raw") => key = Some(binlog_format::DEFAULT_KEY.to_vec()),
            _ => {
                eprintln!("{usage}");
                std::process::exit(2);
            }
        }
        index = 3;
    }
    if args.len() != index + 2 {
        eprintln!("{usage}");
        std::process::exit(2);
    }
    std::process::exit(run_script(&args[index], &args[index + 1], key));
}