//! Judge `td::TQueue` (tddb/td/db/TQueue.h) against the C++ oracle
//! (`tools/fixtures/tqueue_probe.cpp`).
//!
//!   tqueue_probe <script.txt>
//!   tqueue_probe --check <case>
//!
//! The queue semantics live in `rust/src/tqueue.rs`; this file only narrates.
//! Every command prints exactly one line, and what the queue returns -- including
//! `get`'s "size, not event count" and `clear(0)`'s "keep the last slot" -- is
//! the criterion (docs/facts.md F199/F200).
//!
//! The storage is shared through an `Rc<RefCell<_>>` so the probe can call
//! `replay` on it after the queue has taken ownership, exactly like the C++
//! oracle keeps a raw pointer.

#[allow(dead_code)]
#[path = "../binlog_format.rs"]
mod binlog_format;
#[allow(dead_code)]
#[path = "../tqueue.rs"]
mod tqueue;

use std::cell::RefCell;
use std::rc::Rc;

use tqueue::{EventId, RawEvent, StorageCallback, TQueueImpl, TQueueMemoryStorage};

const VALUE_CAP: usize = 32;

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

/// Forwards to the shared `TQueueMemoryStorage` so the probe can still inspect
/// and replay it after handing ownership to the queue.
struct SharedStorage(Rc<RefCell<TQueueMemoryStorage>>);

impl StorageCallback for SharedStorage {
    fn push(&mut self, queue_id: i64, event: &RawEvent) -> u64 {
        self.0.borrow_mut().push(queue_id, event)
    }

    fn pop(&mut self, log_event_id: u64) {
        self.0.borrow_mut().pop(log_event_id);
    }

    fn close(&mut self) {
        self.0.borrow_mut().close();
    }
}

fn parse_i32(text: &str) -> Option<i32> {
    text.parse::<i32>().ok()
}

fn parse_i64(text: &str) -> Option<i64> {
    text.parse::<i64>().ok()
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

fn run_script(script_path: &str) -> i32 {
    let file = match std::fs::File::open(script_path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("cannot read {script_path}: {e}");
            return 2;
        }
    };
    use std::io::BufRead;

    let storage = Rc::new(RefCell::new(TQueueMemoryStorage::new()));
    let mut queue = TQueueImpl::create();
    queue.set_callback(Box::new(SharedStorage(Rc::clone(&storage))));
    let mut result_events: Vec<tqueue::Event> = Vec::new();

    for line in std::io::BufReader::new(file).lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                eprintln!("read error: {e}");
                return 2;
            }
        };
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
                match queue.push(queue_id, data.clone(), expires_at, extra, hint) {
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
                match queue.get(queue_id, from_id, forget != 0, now, max_n as usize, &mut result_events) {
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
                        queue.forget(queue_id, id);
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
                let (deleted, completed) = queue.run_gc(now);
                println!("gc now={now} deleted={deleted} completed={}", i32::from(completed));
            }
            "replay" => {
                if tokens.len() < 2 {
                    println!("reject replay argc={}", tokens.len());
                    continue;
                }
                let mut fresh = TQueueImpl::create();
                storage.borrow().replay(&mut fresh);
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
            }
            "id" => {
                let Some(value) = tokens.get(1).and_then(|t| parse_i32(t)) else {
                    println!("reject id argc={}", tokens.len());
                    continue;
                };
                match EventId::from_int32(value) {
                    Ok(id) => println!(
                        "id n={value} ok=1 value={} valid={} empty={}",
                        id.value(),
                        i32::from(id.is_valid()),
                        i32::from(id.empty())
                    ),
                    Err(e) => println!("id n={value} ok=0 err={e}"),
                }
            }
            "idnext" => {
                let Some(value) = tokens.get(1).and_then(|t| parse_i32(t)) else {
                    println!("reject idnext argc={}", tokens.len());
                    continue;
                };
                match EventId::from_int32(value) {
                    Ok(base) => match base.next() {
                        Ok(id) => println!("idnext n={value} ok=1 value={}", id.value()),
                        Err(e) => println!("idnext n={value} ok=0 err={e}"),
                    },
                    Err(e) => println!("idnext n={value} ok=0 err={e}"),
                }
            }
            "idadv" => {
                let (Some(value), Some(offset)) =
                    (tokens.get(1).and_then(|t| parse_i32(t)), tokens.get(2).and_then(|t| parse_i64(t)))
                else {
                    println!("reject idadv argc={}", tokens.len());
                    continue;
                };
                if offset < 0 {
                    println!("reject idadv argc={}", tokens.len());
                    continue;
                }
                match EventId::from_int32(value) {
                    Ok(base) => match base.advance(offset as usize) {
                        Ok(id) => println!("idadv n={value} off={offset} ok=1 value={}", id.value()),
                        Err(e) => println!("idadv n={value} off={offset} ok=0 err={e}"),
                    },
                    Err(e) => println!("idadv n={value} off={offset} ok=0 err={e}"),
                }
            }
            "idprint" => {
                let Some(value) = tokens.get(1).and_then(|t| parse_i32(t)) else {
                    println!("reject idprint argc={}", tokens.len());
                    continue;
                };
                match EventId::from_int32(value) {
                    Ok(id) => println!("idprint n={value} text={id}"),
                    Err(e) => println!("idprint n={value} ok=0 err={e}"),
                }
            }
            other => println!("reject unknown={other}"),
        }
    }
    0
}

fn run_check(case_name: &str) -> i32 {
    let mut queue = TQueueImpl::create();
    queue.set_callback(Box::new(SharedStorage(Rc::new(RefCell::new(
        TQueueMemoryStorage::new(),
    )))));

    let mut raw = RawEvent {
        expires_at: 1000,
        data: b"x".to_vec(),
        ..RawEvent::default()
    };
    match case_name {
        "invalid_event_id" => {
            raw.event_id = EventId::default();
            let added = queue.do_push(1, raw);
            println!("after added={}", i32::from(added));
        }
        "valid_event_id" => {
            raw.event_id = EventId::from_int32(5).expect("5 is valid");
            let added = queue.do_push(1, raw);
            println!(
                "after added={} size={} head={}",
                i32::from(added),
                queue.get_size(1),
                queue.get_head(1).value()
            );
        }
        other => {
            eprintln!("unknown check case {other}");
            return 2;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--check" {
        std::process::exit(run_check(&args[2]));
    }
    if args.len() != 2 {
        eprintln!("usage: tqueue_probe <script.txt>\n       tqueue_probe --check <case>");
        std::process::exit(2);
    }
    std::process::exit(run_script(&args[1]));
}