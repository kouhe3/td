//! SessionConnection policy on the Rust side: rpc_result / container parsing,
//! the bad_msg_notification action table and the bad_server_salt switch.
//!
//! Oracle: tools/fixtures/session_probe.cpp (same script, drives the real class
//! with a fake transport). Both print one line per op plus callback events:
//!
//! ```text
//! conn session=<u64> salt=<u64>
//! in msg=<u64>
//! event=...
//! salt=<u64>
//! ```
//!
//! Only the inert parts of a session are modelled: check_packet (session id,
//! parity, duplicate window, server-time difference) plus the packet dispatch.
//! Outgoing queries, pings and acks are out of scope.

use std::env;
use std::fs;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const MSG_ID_SCALE: f64 = 4294967296.0; // 1 << 32
const MAX_SAVED: usize = 1000;

const CONTAINER: u32 = 0x73f1_f8dc;
const RPC_RESULT: u32 = 0xf35c_6d01;
const RPC_ERROR: u32 = 0x2144_ca19;
const BAD_MSG_NOTIFICATION: u32 = 0xa7ef_f811;
const BAD_SERVER_SALT: u32 = 0xedab_447b;
const MSGS_ACK: u32 = 0x62d6_b459;
const NEW_SESSION_CREATED: u32 = 0x9ec2_0908;
const VECTOR: u32 = 0x1cb5_c415;
const MSG_DETAILED_INFO: u32 = 0x276d_3ec6;
const MSG_NEW_DETAILED_INFO: u32 = 0x809d_b6df;
const MSGS_ALL_INFO: u32 = 0x8cc0_d131;
const MSGS_STATE_INFO: u32 = 0x04de_b57d;

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn message_id_text(message_id: u64) -> String {
    format!("message 0x{message_id:016x}")
}

struct DuplicateChecker {
    saved: Vec<u64>,
    end: usize,
}

impl DuplicateChecker {
    fn new() -> Self {
        Self { saved: vec![0; 2 * MAX_SAVED], end: 0 }
    }

    fn check(&mut self, message_id: u64) -> Result<(), (i32, String)> {
        if self.end == 2 * MAX_SAVED {
            self.saved.copy_within(MAX_SAVED..2 * MAX_SAVED, 0);
            self.end = MAX_SAVED;
        }
        if self.end == 0 || message_id > self.saved[self.end - 1] {
            self.saved[self.end] = message_id;
            self.end += 1;
            return Ok(());
        }
        if self.end >= MAX_SAVED && message_id < self.saved[0] {
            return Err((
                2,
                format!(
                    "Ignore very old {} older than the oldest known {}",
                    message_id_text(message_id),
                    message_id_text(self.saved[0])
                ),
            ));
        }
        let position = self.saved[..self.end].partition_point(|&saved| saved < message_id);
        if self.saved[position] == message_id {
            return Err((1, format!("Ignore already processed {}", message_id_text(message_id))));
        }
        self.saved.copy_within(position..self.end, position + 1);
        self.saved[position] = message_id;
        self.end += 1;
        Ok(())
    }
}

struct Session {
    salt: u64,
    difference: f64,
    difference_set: bool,
    packet_checker: DuplicateChecker,
    update_checker: DuplicateChecker,
    connected: bool,
    closed: bool,
}

impl Session {
    fn new(_session_id: u64, salt: u64) -> Self {
        Self {
            salt,
            difference: 0.0,
            difference_set: false,
            packet_checker: DuplicateChecker::new(),
            update_checker: DuplicateChecker::new(),
            connected: false,
            closed: false,
        }
    }

    fn server_time(&self, now: f64) -> f64 {
        self.difference + now
    }

    fn set_server_salt(&mut self, salt: u64) {
        self.salt = salt;
    }

    fn update_difference(&mut self, diff: f64) -> bool {
        if !self.difference_set {
            self.difference_set = true;
            self.difference = diff;
        } else if self.difference + 1e-4 < diff {
            self.difference = diff;
        } else {
            return false;
        }
        true
    }

    fn reset_difference(&mut self, diff: f64) {
        self.difference_set = false;
        self.difference = diff;
    }

    fn check_packet(&mut self, message_id: u64, now: f64) -> Result<bool, (i32, String)> {
        if message_id & 1 == 0 {
            return Err((0, format!("Receive invalid {}", message_id_text(message_id))));
        }
        self.packet_checker.check(message_id)?;
        let diff = (message_id >> 32) as u32 as f64 - now;
        let updated = self.update_difference(diff);
        let server_time = self.server_time(now);
        let id_time = message_id as f64 / MSG_ID_SCALE;
        if self.difference_set && !(server_time - 300.0 < id_time && id_time < server_time + 30.0) {
            return Err((0, format!("Ignore too old or too new {}", message_id_text(message_id))));
        }
        Ok(updated)
    }
}

struct Events {
    out: String,
}

impl Events {
    fn new() -> Self {
        Self { out: String::new() }
    }
    fn line(&mut self, text: String) {
        self.out.push_str(&text);
        self.out.push('\n');
    }
    fn on_connected(&mut self) {
        self.line("event=connected".to_string());
    }
    fn on_closed(&mut self, status: &str) {
        self.line(format!("event=closed status={status}"));
    }
    fn on_session_failed(&mut self, status: &str) {
        self.line(format!("event=session_failed status={status}"));
    }
    fn on_message_failed(&mut self, message_id: u64, status: &str) {
        self.line(format!("event=message_failed msg={message_id} status={status}"));
    }
    fn on_time_difference(&mut self, force: bool) {
        self.line(format!("event=time_difference force={}", force as u8));
    }
    fn on_salt_updated(&mut self) {
        self.line("event=server_salt_updated".to_string());
    }
    fn on_ack(&mut self, message_id: u64) {
        self.line(format!("event=ack msg={message_id}"));
    }
    fn on_new_session(&mut self, unique_id: u64, first_message_id: u64) {
        self.line(format!("event=new_session unique={unique_id} first={first_message_id}"));
    }
    fn on_message_info(&mut self, message_id: u64, state: i32, answer: u64, size: i32, source: i32) {
        self.line(format!(
            "event=message_info msg={message_id} state={state} answer={answer} size={size} source={source}"
        ));
    }
    fn on_result_ok(&mut self, req: u64, original: usize, size: usize) {
        self.line(format!("event=result_ok req={req} original={original} size={size}"));
    }
    fn on_result_error(&mut self, req: u64, code: i32, message: &str) {
        self.line(format!("event=result_error req={req} code={code} message={message}"));
    }
    fn on_update(&mut self, size: usize) {
        self.line(format!("event=update size={size}"));
    }
}

/// Little-endian reader that mirrors TlParser's error behaviour for the packet
/// shapes the corpus uses.
struct Reader<'a> {
    data: &'a [u8],
    position: usize,
    error: Option<String>,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0, error: None }
    }
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        if self.position + count > self.data.len() {
            self.error = Some("Not enough data to read".to_string());
            return None;
        }
        let slice = &self.data[self.position..self.position + count];
        self.position += count;
        Some(slice)
    }
    fn int(&mut self) -> Option<u32> {
        self.take(4).map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
    }
    fn long(&mut self) -> Option<u64> {
        self.take(8).map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
    }
    fn string(&mut self) -> Option<String> {
        let len = self.take(1)?[0] as usize;
        let bytes = self.take(len)?;
        let aligned = (1 + len).div_ceil(4) * 4;
        let pad = aligned - 1 - len;
        self.take(pad)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let len = self.take(1)?[0] as usize;
        let bytes = self.take(len)?.to_vec();
        let aligned = (1 + len).div_ceil(4) * 4;
        let pad = aligned - 1 - len;
        self.take(pad)?;
        Some(bytes)
    }
}

struct Driver {
    session: Session,
    events: Events,
}

impl Driver {
    fn new() -> Self {
        Self { session: Session::new(0, 0), events: Events::new() }
    }

    fn inject(&mut self, message_id: u64, seq_no: i32, body: &[u8]) {
        if self.session.closed {
            return;
        }
        let now = now_seconds();
        match self.session.check_packet(message_id, now) {
            Ok(updated) => {
                if updated {
                    self.events.on_time_difference(false);
                }
            }
            Err((1, _)) => return, // duplicate: acked and ignored, no callback
            Err((2, status)) => {
                self.events.on_session_failed("Receive too old packet");
                self.session.closed = true;
                self.events.on_closed(&status);
                return;
            }
            Err((_, status)) => {
                self.session.closed = true;
                self.events.on_closed(&status);
                return;
            }
        }
        if !self.session.connected {
            self.session.connected = true;
            self.events.on_connected();
        }
        if let Err(status) = self.dispatch(message_id, seq_no, body) {
            self.session.closed = true;
            self.events.on_closed(&status);
        }
    }

    fn dispatch(&mut self, message_id: u64, _seq_no: i32, packet: &[u8]) -> Result<(), String> {
        if packet.len() < 4 {
            self.events.on_session_failed("Receive too small packet");
            return Err(format!("Receive packet of size {}", packet.len()));
        }
        let constructor = u32::from_le_bytes(packet[0..4].try_into().unwrap());
        let rest = &packet[4..];
        if constructor == CONTAINER {
            let mut reader = Reader::new(rest);
            let count = reader.int().ok_or("Failed to parse mtproto_api::rpc_container")? as i32;
            for _ in 0..count {
                let child_id = reader.long().ok_or("Failed to parse mtproto_api::message")?;
                let child_seq = reader.int().ok_or("Failed to parse mtproto_api::message")? as i32;
                let size = reader.int().ok_or("Failed to parse mtproto_api::message")? as usize;
                let child = reader.take(size).ok_or("Failed to parse mtproto_api::message")?;
                self.dispatch(child_id, child_seq, child)?;
            }
            return Ok(());
        }
        if constructor == RPC_RESULT {
            return self.on_rpc_result(message_id, rest.len() + 4, rest);
        }
        if constructor == BAD_SERVER_SALT {
            let mut reader = Reader::new(rest);
            let bad_msg_id = reader.long().ok_or("Failed to parse mtproto_api::bad_server_salt")?;
            let _seq = reader.int();
            let _code = reader.int();
            let new_salt = reader.long().ok_or("Failed to parse mtproto_api::bad_server_salt")?;
            self.session.set_server_salt(new_salt);
            self.events.on_salt_updated();
            self.events.on_message_failed(bad_msg_id, "Bad server salt");
            return Ok(());
        }
        if constructor == BAD_MSG_NOTIFICATION {
            let mut reader = Reader::new(rest);
            let bad_msg_id = reader.long().ok_or("Failed to parse mtproto_api::bad_msg_notification")?;
            let _seq = reader.int();
            let code = reader.int().ok_or("Failed to parse mtproto_api::bad_msg_notification")? as i32;
            return self.on_bad_msg(message_id, bad_msg_id, code);
        }
        if constructor == MSGS_ACK {
            let mut reader = Reader::new(rest);
            let vector = reader.int().ok_or("Failed to parse mtproto_api::msgs_ack")?;
            if vector != VECTOR {
                return Err("Failed to parse mtproto_api::msgs_ack".to_string());
            }
            let count = reader.int().ok_or("Failed to parse mtproto_api::msgs_ack")? as i32;
            for _ in 0..count {
                let message_id = reader.long().ok_or("Failed to parse mtproto_api::msgs_ack")?;
                self.events.on_ack(message_id);
            }
            return Ok(());
        }
        if constructor == NEW_SESSION_CREATED {
            let mut reader = Reader::new(rest);
            let first_message_id = reader.long().ok_or("Failed to parse mtproto_api::new_session_created")?;
            let unique_id = reader.long().ok_or("Failed to parse mtproto_api::new_session_created")?;
            let _salt = reader.long().ok_or("Failed to parse mtproto_api::new_session_created")?;
            self.events.on_new_session(unique_id, first_message_id);
            return Ok(());
        }
        if constructor == MSG_DETAILED_INFO {
            let mut reader = Reader::new(rest);
            let msg_id = reader.long().ok_or("Failed to parse mtproto_api::msg_detailed_info")?;
            let answer_msg_id = reader.long().ok_or("Failed to parse mtproto_api::msg_detailed_info")?;
            let bytes = reader.int().ok_or("Failed to parse mtproto_api::msg_detailed_info")? as i32;
            let status = reader.int().ok_or("Failed to parse mtproto_api::msg_detailed_info")? as i32;
            self.events.on_message_info(msg_id, status, answer_msg_id, bytes, 2);
            return Ok(());
        }
        if constructor == MSG_NEW_DETAILED_INFO {
            let mut reader = Reader::new(rest);
            let answer_msg_id = reader.long().ok_or("Failed to parse mtproto_api::msg_new_detailed_info")?;
            let bytes = reader.int().ok_or("Failed to parse mtproto_api::msg_new_detailed_info")? as i32;
            let status = reader.int().ok_or("Failed to parse mtproto_api::msg_new_detailed_info")? as i32;
            let _ = status;
            self.events.on_message_info(0, 0, answer_msg_id, bytes, 0);
            return Ok(());
        }
        if constructor == MSGS_ALL_INFO {
            let mut reader = Reader::new(rest);
            let vector = reader.int().ok_or("Failed to parse mtproto_api::msgs_all_info")?;
            if vector != VECTOR {
                return Err("Failed to parse mtproto_api::msgs_all_info".to_string());
            }
            let count = reader.int().ok_or("Failed to parse mtproto_api::msgs_all_info")? as usize;
            let mut ids = Vec::with_capacity(count);
            for _ in 0..count {
                ids.push(reader.long().ok_or("Failed to parse mtproto_api::msgs_all_info")?);
            }
            let info = reader.bytes().ok_or("Failed to parse mtproto_api::msgs_all_info")?;
            if ids.len() != info.len() {
                return Err("message count mismatch".to_string());
            }
            for (index, id) in ids.iter().enumerate() {
                self.events.on_message_info(*id, info[index] as i32, 0, 0, 1);
            }
            return Ok(());
        }
        if constructor == MSGS_STATE_INFO {
            // No service query is ever registered in this model.
            return Err("Unknown msgs_state_info".to_string());
        }
        // Unknown constructor: treat as an update.
        match self.session.update_checker.check(message_id) {
            Ok(()) => {}
            Err((2, status)) => {
                self.events.on_session_failed(&status);
                return Err(status);
            }
            Err(_) => return Ok(()),
        }
        self.events.on_update(packet.len());
        Ok(())
    }

    fn on_rpc_result(&mut self, outer_message_id: u64, original: usize, rest: &[u8]) -> Result<(), String> {
        let mut reader = Reader::new(rest);
        let req_msg_id = reader.long().ok_or("Failed to parse mtproto_api::rpc_result")?;
        if req_msg_id == 0 {
            return Err("Receive an update in rpc_result".to_string());
        }
        // uint64 arithmetic, like TDLib: a small req_msg_id wraps and the branch fires.
        if outer_message_id < req_msg_id.wrapping_sub(15u64 << 32) {
            let now = now_seconds();
            let diff = (outer_message_id >> 32) as u32 as f64 - now;
            self.session.reset_difference(diff);
            self.events.on_time_difference(true);
        }
        let constructor = reader.int().ok_or("Failed to parse mtproto_api::rpc_result")?;
        if constructor == RPC_ERROR {
            let code = reader.int().ok_or("Failed to parse mtproto_api::rpc_error")? as i32;
            let message = reader.string().ok_or("Failed to parse mtproto_api::rpc_error")?;
            self.events.on_result_error(req_msg_id, code, &message);
            return Ok(());
        }
        // Default branch passes the whole remaining packet through.
        let payload_size = original.saturating_sub(12);
        self.events.on_result_ok(req_msg_id, original, payload_size);
        Ok(())
    }

    fn on_bad_msg(&mut self, outer_message_id: u64, bad_msg_id: u64, code: i32) -> Result<(), String> {
        match code {
            16 | 20 => {
                let status = if code == 16 { "MessageId is too low" } else { "MessageId is too old" };
                self.events.on_message_failed(bad_msg_id, status);
                Ok(())
            }
            17 => {
                let now = now_seconds();
                let diff = (outer_message_id >> 32) as u32 as f64 - now;
                self.session.reset_difference(diff);
                self.events.on_time_difference(true);
                self.events.on_session_failed("MessageId is too high");
                Err("MessageId is too high".to_string())
            }
            18 => {
                Err("MessageId is not divisible by 4".to_string())
            }
            19 => Err("Container and older message MessageId collision".to_string()),
            32 => Err("SeqNo is too low".to_string()),
            33 => Err("SeqNo is too high".to_string()),
            34 => Err("SeqNo is not even for an irrelevant message".to_string()),
            35 => Err("SeqNo is not odd for a relevant message".to_string()),
            64 => Err("Invalid Container".to_string()),
            _ => Err("Unknown error code".to_string()),
        }
    }
}

fn unhex(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index + 1 < bytes.len() {
        let high = (bytes[index] as char).to_digit(16).unwrap_or(0) as u8;
        let low = (bytes[index + 1] as char).to_digit(16).unwrap_or(0) as u8;
        out.push((high << 4) | low);
        index += 2;
    }
    out
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: session_probe <script.txt>");
        return ExitCode::from(2);
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read {path}: {error}");
            return ExitCode::from(1);
        }
    };
    let mut driver = Driver::new();
    let mut out = String::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        match tokens[0] {
            "newconn" => {
                let session_id: u64 = tokens[1].parse().unwrap_or(0);
                let salt: u64 = tokens[2].parse().unwrap_or(0);
                driver = Driver::new();
                driver.session = Session::new(session_id, salt);
                out.push_str(&format!("conn session={session_id} salt={salt}\n"));
            }
            "in" => {
                let message_id: u64 = tokens[1].parse().unwrap_or(0);
                let seq_no: i32 = tokens[2].parse().unwrap_or(0);
                let body = unhex(tokens[3]);
                out.push_str(&format!("in msg={message_id}\n"));
                driver.inject(message_id, seq_no, &body);
                out.push_str(&driver.events.out);
                driver.events.out.clear();
            }
            "salt" => {
                out.push_str(&format!("salt={}\n", driver.session.salt));
            }
            other => {
                eprintln!("unknown op {other} at line {}", index + 1);
                return ExitCode::from(2);
            }
        }
    }
    print!("{out}");
    ExitCode::SUCCESS
}