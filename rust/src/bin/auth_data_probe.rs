//! AuthData's salt / server-time / packet state machine, with an injected clock.
//!
//! Oracle: tools/fixtures/auth_data_probe.cpp (links the built libtdmtproto.a).
//! Both probes read the same script and must print the same lines:
//!
//! ```text
//! session <u64> | set <salt> <now> | get <now> | valid <now> | has <now>
//! need <now> | future <now> <salt> <since> <until> ...
//! diff <d> | reset <d> | servertime <now>
//! out <msg_id> <now> | in <msg_id> <now>
//! packet <session> <msg_id> <now>
//! ```
//!
//! Doubles are printed as their 64-bit pattern, so no float formatting drifts.
//! The initial salt of a fresh AuthData is random in TDLib; scripts always call
//! `set` before a value can be observed.
//!
//! Usage: auth_data_probe <script.txt>

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::process::ExitCode;

const MSG_ID_SCALE: f64 = 4294967296.0; // 1 << 32
const MAX_SAVED_MESSAGE_IDS: usize = 1000;

struct ServerSalt {
    salt: u64,
    valid_since: f64,
    valid_until: f64,
}

struct MessageIdDuplicateChecker {
    saved: Vec<u64>,
    end: usize,
}

impl MessageIdDuplicateChecker {
    fn new() -> Self {
        Self { saved: vec![0; 2 * MAX_SAVED_MESSAGE_IDS], end: 0 }
    }

    fn check(&mut self, message_id: u64) -> Result<(), (i32, String)> {
        if self.end == 2 * MAX_SAVED_MESSAGE_IDS {
            self.saved.copy_within(MAX_SAVED_MESSAGE_IDS..2 * MAX_SAVED_MESSAGE_IDS, 0);
            self.end = MAX_SAVED_MESSAGE_IDS;
        }
        if self.end == 0 || message_id > self.saved[self.end - 1] {
            self.saved[self.end] = message_id;
            self.end += 1;
            return Ok(());
        }
        if self.end >= MAX_SAVED_MESSAGE_IDS && message_id < self.saved[0] {
            return Err((
                2,
                format!(
                    "Ignore very old {} older than the oldest known {}",
                    format_message_id(message_id),
                    format_message_id(self.saved[0])
                ),
            ));
        }
        let position = self.saved[..self.end].partition_point(|&saved| saved < message_id);
        if self.saved[position] == message_id {
            return Err((1, format!("Ignore already processed {}", format_message_id(message_id))));
        }
        self.saved.copy_within(position..self.end, position + 1);
        self.saved[position] = message_id;
        self.end += 1;
        Ok(())
    }
}

fn format_message_id(message_id: u64) -> String {
    format!("message 0x{message_id:016x}")
}

struct AuthData {
    server_time_difference_was_updated: bool,
    server_time_difference: f64,
    server_salt: ServerSalt,
    session_id: u64,
    future_salts: Vec<ServerSalt>,
    duplicate_checker: MessageIdDuplicateChecker,
}

impl AuthData {
    fn new() -> Self {
        Self {
            server_time_difference_was_updated: false,
            server_time_difference: 0.0,
            // TDLib seeds this with Random::secure_int64(); no script may observe it
            // before set_server_salt replaces it.
            server_salt: ServerSalt { salt: 0, valid_since: -1e10, valid_until: -1e10 },
            session_id: 0,
            future_salts: Vec::new(),
            duplicate_checker: MessageIdDuplicateChecker::new(),
        }
    }

    fn get_server_time(&self, now: f64) -> f64 {
        self.server_time_difference + now
    }

    fn set_server_salt(&mut self, salt: u64, now: f64) {
        self.server_salt.salt = salt;
        let server_time = self.get_server_time(now);
        self.server_salt.valid_since = server_time;
        self.server_salt.valid_until = server_time + 60.0 * 10.0;
        self.future_salts.clear();
    }

    fn is_server_salt_valid(&self, now: f64) -> bool {
        self.server_salt.valid_until > self.get_server_time(now) + 60.0
    }

    fn update_salt(&mut self, now: f64) {
        let server_time = self.get_server_time(now);
        while let Some(last) = self.future_salts.last() {
            if last.valid_since < server_time {
                let adopted = self.future_salts.pop().unwrap();
                self.server_salt = adopted;
            } else {
                break;
            }
        }
    }

    fn get_server_salt(&mut self, now: f64) -> u64 {
        self.update_salt(now);
        self.server_salt.salt
    }

    fn has_salt(&mut self, now: f64) -> bool {
        self.update_salt(now);
        self.is_server_salt_valid(now)
    }

    fn need_future_salts(&mut self, now: f64) -> bool {
        self.update_salt(now);
        self.future_salts.is_empty() || !self.is_server_salt_valid(now)
    }

    fn set_future_salts(&mut self, mut salts: Vec<ServerSalt>, now: f64) {
        if salts.is_empty() {
            return;
        }
        // TDLib sorts with `a.valid_since > b.valid_since` and then pops from the back,
        // i.e. the smallest valid_since is adopted first. Equal keys are avoided by the
        // script generator so the unstable sort order cannot matter.
        salts.sort_by(|a, b| b.valid_since.partial_cmp(&a.valid_since).unwrap_or(std::cmp::Ordering::Equal));
        self.future_salts = salts;
        self.update_salt(now);
    }

    fn get_future_salts(&self) -> Vec<(u64, f64, f64)> {
        let mut result: Vec<(u64, f64, f64)> = self
            .future_salts
            .iter()
            .map(|salt| (salt.salt, salt.valid_since, salt.valid_until))
            .collect();
        result.push((self.server_salt.salt, self.server_salt.valid_since, self.server_salt.valid_until));
        result
    }

    fn update_server_time_difference(&mut self, diff: f64) -> bool {
        if !self.server_time_difference_was_updated {
            self.server_time_difference_was_updated = true;
            self.server_time_difference = diff;
        } else if self.server_time_difference + 1e-4 < diff {
            self.server_time_difference = diff;
        } else {
            return false;
        }
        true
    }

    fn reset_server_time_difference(&mut self, diff: f64) {
        self.server_time_difference_was_updated = false;
        self.server_time_difference = diff;
    }

    fn is_valid_outbound_msg_id(&self, message_id: u64, now: f64) -> bool {
        let server_time = self.get_server_time(now);
        let id_time = message_id as f64 / MSG_ID_SCALE;
        server_time - 150.0 < id_time && id_time < server_time + 30.0
    }

    fn is_valid_inbound_msg_id(&self, message_id: u64, now: f64) -> bool {
        let server_time = self.get_server_time(now);
        let id_time = message_id as f64 / MSG_ID_SCALE;
        server_time - 300.0 < id_time && id_time < server_time + 30.0
    }

    /// Returns the `time_difference_was_updated` flag plus the status, mirroring
    /// TDLib's out-parameter: the flag is meaningful even when the check fails.
    fn check_packet(&mut self, session_id: u64, message_id: u64, now: f64) -> (bool, Result<(), (i32, String)>) {
        if self.session_id != session_id {
            return (
                false,
                Err((
                    0,
                    format!(
                        "Receive packet from different session {} in session {}",
                        session_id, self.session_id
                    ),
                )),
            );
        }
        if message_id & 1 == 0 {
            return (false, Err((0, format!("Receive invalid {}", format_message_id(message_id)))));
        }
        if let Err(error) = self.duplicate_checker.check(message_id) {
            return (false, Err(error));
        }
        let diff = (message_id >> 32) as u32 as f64 - now;
        let updated = self.update_server_time_difference(diff);
        if self.server_time_difference_was_updated && !self.is_valid_inbound_msg_id(message_id, now) {
            return (updated, Err((0, format!("Ignore too old or too new {}", format_message_id(message_id)))));
        }
        (updated, Ok(()))
    }
}

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn run_line(data: &mut AuthData, tokens: &[&str], out: &mut String) -> Result<(), String> {
    let op = tokens[0];
    let number = |index: usize| -> Result<u64, String> {
        tokens
            .get(index)
            .ok_or_else(|| format!("{op}: missing argument {index}"))?
            .parse::<u64>()
            .map_err(|error| format!("{op}: bad u64 at {index}: {error}"))
    };
    let real = |index: usize| -> Result<f64, String> {
        tokens
            .get(index)
            .ok_or_else(|| format!("{op}: missing argument {index}"))?
            .parse::<f64>()
            .map_err(|error| format!("{op}: bad f64 at {index}: {error}"))
    };
    match op {
        "session" => {
            data.session_id = number(1)?;
            out.push_str("session ok\n");
        }
        "set" => {
            let salt = number(1)?;
            data.set_server_salt(salt, real(2)?);
            out.push_str("set ok\n");
        }
        "get" => {
            let salt = data.get_server_salt(real(1)?);
            let _ = writeln!(out, "get salt={salt}");
        }
        "valid" => {
            let result = data.is_server_salt_valid(real(1)?);
            let _ = writeln!(out, "valid {}", result as u8);
        }
        "has" => {
            let result = data.has_salt(real(1)?);
            let _ = writeln!(out, "has {}", result as u8);
        }
        "need" => {
            let result = data.need_future_salts(real(1)?);
            let _ = writeln!(out, "need {}", result as u8);
        }
        "future" => {
            let now = real(1)?;
            let rest = &tokens[2..];
            if rest.len() % 3 != 0 {
                return Err("future: arguments must be <salt> <since> <until> triples".to_string());
            }
            let mut salts = Vec::with_capacity(rest.len() / 3);
            for triple in rest.chunks(3) {
                let salt = triple[0].parse::<u64>().map_err(|error| format!("future: bad salt: {error}"))?;
                let valid_since =
                    triple[1].parse::<f64>().map_err(|error| format!("future: bad since: {error}"))?;
                let valid_until =
                    triple[2].parse::<f64>().map_err(|error| format!("future: bad until: {error}"))?;
                salts.push(ServerSalt { salt, valid_since, valid_until });
            }
            data.set_future_salts(salts, now);
            let result = data.get_future_salts();
            let _ = write!(out, "future n={}", result.len());
            for (salt, valid_since, valid_until) in result {
                let _ = write!(out, " {salt}:{}:{}", bits(valid_since), bits(valid_until));
            }
            out.push('\n');
        }
        "diff" => {
            let updated = data.update_server_time_difference(real(1)?);
            let _ = writeln!(
                out,
                "diff updated={} cur={}",
                updated as u8,
                bits(data.server_time_difference)
            );
        }
        "reset" => {
            data.reset_server_time_difference(real(1)?);
            let _ = writeln!(out, "reset cur={}", bits(data.server_time_difference));
        }
        "servertime" => {
            let _ = writeln!(out, "servertime t={}", bits(data.get_server_time(real(1)?)));
        }
        "out" | "in" => {
            let message_id = number(1)?;
            let now = real(2)?;
            let valid = if op == "out" {
                data.is_valid_outbound_msg_id(message_id, now)
            } else {
                data.is_valid_inbound_msg_id(message_id, now)
            };
            let _ = writeln!(out, "{op} {}", valid as u8);
        }
        "packet" => {
            let session_id = number(1)?;
            let message_id = number(2)?;
            let now = real(3)?;
            let (updated, status) = data.check_packet(session_id, message_id, now);
            match status {
                Ok(()) => {
                    let _ = writeln!(out, "packet ok updated={}", updated as u8);
                }
                Err((code, text)) => {
                    let _ = writeln!(out, "packet err={code} updated={} text={text}", updated as u8);
                }
            }
        }
        other => return Err(format!("unknown script op {other}")),
    }
    Ok(())
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: auth_data_probe <script.txt>");
        return ExitCode::from(2);
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read {path}: {error}");
            return ExitCode::from(1);
        }
    };

    let mut data = AuthData::new();
    let mut out = String::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if let Err(error) = run_line(&mut data, &tokens, &mut out) {
            eprintln!("script error at line {}: {error}", index + 1);
            return ExitCode::from(1);
        }
    }
    print!("{out}");
    ExitCode::SUCCESS
}