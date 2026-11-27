//! Reproduce AuthData::next_seq_no and the structure of next_message_id.
//!
//! `next_seq_no` is a deterministic state machine: content-related messages get
//! an odd number (and advance by 2), service messages get the current even one.
//!
//! `next_message_id` mixes `server_time << 32` with Random::secure_int32, so its
//! low 22 bits are random by design. Only the high 32 bits, `% 4 == 0` and
//! monotonicity are comparable with the oracle; this probe feeds a fixed
//! pseudo-random stream so its own output is reproducible.
//!
//! Usage: msgid_probe <fixture.json>
//! Output: seq=<csv>, msg_id=<csv>, invariants_ok=<true|false>

use std::env;
use std::fs;
use std::process::ExitCode;

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

struct SeqState {
    value: i32,
}

impl SeqState {
    fn next(&mut self, content: bool) -> i32 {
        let mut res = self.value;
        if content {
            res |= 1;
            self.value += 2;
        }
        res
    }
}

/// Same shape as AuthData::next_message_id, with an injectable random word.
fn next_message_id(last: &mut u64, now: f64, random: u32) -> u64 {
    let mut t = (now * ((1u64 << 32) as f64)) as u64;
    t ^= (random & ((1 << 22) - 1)) as u64;
    let mut result = t & !3u64;
    if *last >= result {
        let to_mul = ((random >> 22) & 1023) as u64 + 1;
        result = *last + 8 * to_mul;
    }
    *last = result;
    result
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: msgid_probe <fixture.json>");
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

    let now: f64 = get("now").parse().unwrap();
    let count: usize = get("count").parse().unwrap();

    let mut seq = SeqState { value: 0 };
    let mut last_id = 0u64;
    let mut seqs = Vec::new();
    let mut ids = Vec::new();
    let mut rnd: u32 = 0x1234_5678;
    for i in 0..count {
        let content = i % 2 == 0;
        seqs.push(seq.next(content).to_string());
        rnd = rnd.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ids.push(next_message_id(&mut last_id, now, rnd));
    }

    let high = ((now * ((1u64 << 32) as f64)) as u64) >> 32;
    let mut invariants_ok = true;
    let mut prev = 0u64;
    for id in &ids {
        invariants_ok &= id % 4 == 0;
        invariants_ok &= *id > prev;
        invariants_ok &= (id >> 32) == high;
        prev = *id;
    }

    println!("seq={}", seqs.join(","));
    println!("msg_id={}", ids.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));
    println!("invariants_ok={invariants_ok}");
    ExitCode::SUCCESS
}