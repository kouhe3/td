//! Generated-class encoding for the maintenance/service messages, plus the two
//! abstract dispatch points, byte-compatible with TDLib's `mtproto_api`.
//!
//! Oracle: tools/fixtures/svc_msg_probe.cpp (same CLI, links the generated
//! classes from libtdmtproto.a). `store` writes only the fields; `full`
//! prepends the constructor id, exactly like the boxed wrapper does.
//!
//! ```text
//! svc_msg_probe bmn <bad_msg_id> <bad_msg_seqno> <error_code>
//! svc_msg_probe bss <bad_msg_id> <bad_msg_seqno> <error_code> <new_server_salt>
//! svc_msg_probe nsc <first_msg_id> <unique_id> <server_salt>
//! svc_msg_probe pong <msg_id> <ping_id>
//! svc_msg_probe resend <id64,id64,...>
//! svc_msg_probe mdinfo <msg_id> <answer_msg_id> <bytes> <status>
//! svc_msg_probe mdnew <answer_msg_id> <bytes> <status>
//! svc_msg_probe parse_bmn <bytes_hex>
//! svc_msg_probe parse_mdi <bytes_hex>
//! ```

use std::env;
use std::fmt::Write as _;
use std::process::ExitCode;

const BAD_MSG_NOTIFICATION: u32 = 0xa7ef_f811;
const BAD_SERVER_SALT: u32 = 0xedab_447b;
const NEW_SESSION_CREATED: u32 = 0x9ec2_0908;
const PONG: u32 = 0x3477_73c5;
const MSG_RESEND_REQ: u32 = 0x7d86_1a08;
const MSG_DETAILED_INFO: u32 = 0x276d_3ec6;
const MSG_NEW_DETAILED_INFO: u32 = 0x809d_b6df;
const VECTOR_ID: u32 = 0x1cb5_c415;

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

fn unhex(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index + 1 < bytes.len() {
        let pair = std::str::from_utf8(&bytes[index..index + 2]).unwrap_or("00");
        out.push(u8::from_str_radix(pair, 16).unwrap_or(0));
        index += 2;
    }
    out
}

fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn store_and_print(id: u32, fields: &[u8]) {
    let mut full = Vec::with_capacity(4 + fields.len());
    full.extend_from_slice(&id.to_le_bytes());
    full.extend_from_slice(fields);
    println!("id={id:08x} fields={} full={}", hex_encode(fields), hex_encode(&full));
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
    error: Option<&'static str>,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0, error: None }
    }

    fn take(&mut self, count: usize) -> &'a [u8] {
        if self.error.is_some() || self.position + count > self.data.len() {
            self.error = Some("Not enough data to read");
            return &[];
        }
        let slice = &self.data[self.position..self.position + count];
        self.position += count;
        slice
    }

    fn int(&mut self) -> i32 {
        let bytes = self.take(4);
        if bytes.is_empty() {
            return 0;
        }
        i32::from_le_bytes(bytes.try_into().unwrap())
    }

    fn long(&mut self) -> i64 {
        let bytes = self.take(8);
        if bytes.is_empty() {
            return 0;
        }
        i64::from_le_bytes(bytes.try_into().unwrap())
    }

    /// TDLib's `fetch_end`: leftover bytes are an error, but only if no earlier
    /// error already zeroed the remaining length.
    fn finish(&mut self) {
        if self.error.is_none() && self.position != self.data.len() {
            self.error = Some("Too much data to fetch");
        }
    }
}

fn parse_bmn(hex: &str) -> String {
    let data = unhex(hex);
    let mut reader = Reader::new(&data);
    let constructor = reader.int() as u32;
    let mut out = String::new();
    if reader.error.is_some() {
        reader.finish();
        let _ = writeln!(out, "status=err text={}", reader.error.unwrap());
        return out;
    }
    match constructor {
        BAD_SERVER_SALT => {
            let bad_msg_id = reader.long();
            let bad_msg_seqno = reader.int();
            let error_code = reader.int();
            let new_server_salt = reader.long();
            reader.finish();
            if let Some(error) = reader.error {
                let _ = writeln!(out, "status=err text={error}");
                return out;
            }
            out.push_str("type=bad_server_salt\n");
            let _ = writeln!(out, "bad_msg_id={bad_msg_id}");
            let _ = writeln!(out, "bad_msg_seqno={bad_msg_seqno}");
            let _ = writeln!(out, "error_code={error_code}");
            let _ = writeln!(out, "new_server_salt={new_server_salt}");
        }
        BAD_MSG_NOTIFICATION => {
            let bad_msg_id = reader.long();
            let bad_msg_seqno = reader.int();
            let error_code = reader.int();
            reader.finish();
            if let Some(error) = reader.error {
                let _ = writeln!(out, "status=err text={error}");
                return out;
            }
            out.push_str("type=bad_msg_notification\n");
            let _ = writeln!(out, "bad_msg_id={bad_msg_id}");
            let _ = writeln!(out, "bad_msg_seqno={bad_msg_seqno}");
            let _ = writeln!(out, "error_code={error_code}");
        }
        other => {
            let _ = writeln!(out, "status=err text=Unknown constructor found 0x{other:08x}");
        }
    }
    out
}

fn parse_mdi(hex: &str) -> String {
    let data = unhex(hex);
    let mut reader = Reader::new(&data);
    let constructor = reader.int() as u32;
    let mut out = String::new();
    if reader.error.is_some() {
        reader.finish();
        let _ = writeln!(out, "status=err text={}", reader.error.unwrap());
        return out;
    }
    match constructor {
        MSG_DETAILED_INFO => {
            let msg_id = reader.long();
            let answer_msg_id = reader.long();
            let bytes = reader.int();
            let status = reader.int();
            reader.finish();
            if let Some(error) = reader.error {
                let _ = writeln!(out, "status=err text={error}");
                return out;
            }
            out.push_str("type=msg_detailed_info\n");
            let _ = writeln!(out, "msg_id={msg_id}");
            let _ = writeln!(out, "answer_msg_id={answer_msg_id}");
            let _ = writeln!(out, "bytes={bytes}");
            let _ = writeln!(out, "status_code={status}");
        }
        MSG_NEW_DETAILED_INFO => {
            let answer_msg_id = reader.long();
            let bytes = reader.int();
            let status = reader.int();
            reader.finish();
            if let Some(error) = reader.error {
                let _ = writeln!(out, "status=err text={error}");
                return out;
            }
            out.push_str("type=msg_new_detailed_info\n");
            let _ = writeln!(out, "answer_msg_id={answer_msg_id}");
            let _ = writeln!(out, "bytes={bytes}");
            let _ = writeln!(out, "status_code={status}");
        }
        other => {
            let _ = writeln!(out, "status=err text=Unknown constructor found 0x{other:08x}");
        }
    }
    out
}

fn argument(args: &[String], index: usize) -> String {
    args.get(index).cloned().unwrap_or_default()
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: svc_msg_probe <mode> ...");
        return ExitCode::from(2);
    }
    let mode = args[1].as_str();
    let i64_at = |index: usize| -> i64 { argument(&args, index).parse().unwrap_or(0) };
    let i32_at = |index: usize| -> i32 { argument(&args, index).parse().unwrap_or(0) };

    let mut fields = Vec::new();
    let id = match mode {
        "bmn" => {
            push_i64(&mut fields, i64_at(2));
            push_i32(&mut fields, i32_at(3));
            push_i32(&mut fields, i32_at(4));
            BAD_MSG_NOTIFICATION
        }
        "bss" => {
            push_i64(&mut fields, i64_at(2));
            push_i32(&mut fields, i32_at(3));
            push_i32(&mut fields, i32_at(4));
            push_i64(&mut fields, i64_at(5));
            BAD_SERVER_SALT
        }
        "nsc" => {
            push_i64(&mut fields, i64_at(2));
            push_i64(&mut fields, i64_at(3));
            push_i64(&mut fields, i64_at(4));
            NEW_SESSION_CREATED
        }
        "pong" => {
            push_i64(&mut fields, i64_at(2));
            push_i64(&mut fields, i64_at(3));
            PONG
        }
        "resend" => {
            let mut ids = Vec::new();
            for token in argument(&args, 2).split(',').filter(|token| !token.is_empty()) {
                ids.push(token.parse::<i64>().unwrap_or(0));
            }
            push_i32(&mut fields, VECTOR_ID as i32);
            push_i32(&mut fields, ids.len() as i32);
            for value in ids {
                push_i64(&mut fields, value);
            }
            MSG_RESEND_REQ
        }
        "mdinfo" => {
            push_i64(&mut fields, i64_at(2));
            push_i64(&mut fields, i64_at(3));
            push_i32(&mut fields, i32_at(4));
            push_i32(&mut fields, i32_at(5));
            MSG_DETAILED_INFO
        }
        "mdnew" => {
            push_i64(&mut fields, i64_at(2));
            push_i32(&mut fields, i32_at(3));
            push_i32(&mut fields, i32_at(4));
            MSG_NEW_DETAILED_INFO
        }
        "parse_bmn" => {
            print!("{}", parse_bmn(&argument(&args, 2)));
            return ExitCode::SUCCESS;
        }
        "parse_mdi" => {
            print!("{}", parse_mdi(&argument(&args, 2)));
            return ExitCode::SUCCESS;
        }
        other => {
            eprintln!("bad arguments for mode {other}");
            return ExitCode::from(2);
        }
    };
    store_and_print(id, &fields);
    ExitCode::SUCCESS
}