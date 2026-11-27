//! Build the RPC request header: invokeWithLayer + initConnection.
//!
//! TDLib hand-writes this in td/telegram/net/MtprotoHeader.cpp. The oracle is
//! that class (tools/fixtures/rpc_probe.cpp); this probe reproduces the bytes:
//!
//!   invokeWithLayer#da9b0d0d layer:int
//!     initConnection#c1cd5ea9 flags:# api_id:int device_model:string
//!       system_version:string app_version:string system_lang_code:string
//!       lang_pack:string lang_code:string params:JSONValue
//!
//! With an empty `parameters` the params field is a jsonObject holding a single
//! jsonObjectValue("tz_offset", jsonNumber(0.0)).
//!
//! Usage: rpc_probe <fixture.json>
//! Output: header=<hex>

use std::env;
use std::fs;
use std::process::ExitCode;

const INVOKE_WITH_LAYER: u32 = 0xda9b_0d0d;
const INIT_CONNECTION: u32 = 0xc1cd_5ea9;
const JSON_OBJECT: u32 = 0x99c1_d49d;
const VECTOR: u32 = 0x1cb5_c415;
const JSON_OBJECT_VALUE: u32 = 0xc0de_1bd9;
const JSON_NUMBER: u32 = 0x2be0_dfa4;
const LAYER: i32 = 234;

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

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

fn push_tl_string(out: &mut Vec<u8>, s: &[u8]) {
    assert!(s.len() < 254, "long strings are not needed here");
    out.push(s.len() as u8);
    out.extend_from_slice(s);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: rpc_probe <fixture.json>");
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

    let api_id: i32 = get("api_id").parse().unwrap();
    let device = get("device_model").as_bytes().to_vec();
    let system = get("system_version").as_bytes().to_vec();
    let app = get("application_version").as_bytes().to_vec();
    let lang = get("system_language_code").as_bytes().to_vec();

    let mut out = Vec::new();
    out.extend_from_slice(&INVOKE_WITH_LAYER.to_le_bytes());
    out.extend_from_slice(&LAYER.to_le_bytes());
    out.extend_from_slice(&INIT_CONNECTION.to_le_bytes());
    // flags = 1 << 1 (params present, proxy absent)
    out.extend_from_slice(&2i32.to_le_bytes());
    out.extend_from_slice(&api_id.to_le_bytes());
    push_tl_string(&mut out, &device);
    push_tl_string(&mut out, &system);
    push_tl_string(&mut out, &app);
    push_tl_string(&mut out, &lang);
    // language_pack empty -> both language fields are empty strings
    push_tl_string(&mut out, b"");
    push_tl_string(&mut out, b"");

    // params: jsonObject with one tz_offset entry
    out.extend_from_slice(&JSON_OBJECT.to_le_bytes());
    out.extend_from_slice(&VECTOR.to_le_bytes());
    out.extend_from_slice(&1i32.to_le_bytes());
    out.extend_from_slice(&JSON_OBJECT_VALUE.to_le_bytes());
    push_tl_string(&mut out, b"tz_offset");
    out.extend_from_slice(&JSON_NUMBER.to_le_bytes());
    out.extend_from_slice(&0f64.to_le_bytes());

    println!("header={}", hex_encode(&out));
    ExitCode::SUCCESS
}