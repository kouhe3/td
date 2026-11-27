//! MtprotoHeader option matrix (default and anonymous headers).
//!
//! Oracle: tools/fixtures/header_probe.cpp (same argv, links MtprotoHeader).
//! Reproduces the hand-written TL from td/telegram/net/MtprotoHeader.cpp:
//!
//! ```text
//! invokeWithLayer#da9b0d0d layer=234
//!   initConnection#c1cd5ea9 flags api_id device_model system_version
//!     app_version system_lang_code lang_pack lang_code params:JSONValue
//! ```
//!
//! flags: 1<<0 MTProxy present (ignored for anonymous), 1<<1 non-anonymous,
//! 1<<10 emulator. Empty `parameters` only: params is a jsonObject with one
//! jsonObjectValue("tz_offset", jsonNumber(tz_offset)). language_pack non-empty
//! and language_code not starting with 'X' writes (pack, code or "en");
//! everything else writes two empty strings.
//!
//! Usage: header_probe <api_id> <device> <system> <app> <sys_lang> <lang_pack>
//!        <lang_code> <tz_offset> <emulator> <proxy> [server] [port]

use std::env;
use std::process::ExitCode;

const INVOKE_WITH_LAYER: u32 = 0xda9b_0d0d;
const INIT_CONNECTION: u32 = 0xc1cd_5ea9;
const INPUT_CLIENT_PROXY: u32 = 0x7558_8b3f;
const JSON_OBJECT: u32 = 0x99c1_d49d;
const VECTOR: u32 = 0x1cb5_c415;
const JSON_OBJECT_VALUE: u32 = 0xc0de_1bd9;
const JSON_NUMBER: u32 = 0x2be0_dfa4;
const LAYER: i32 = 234;

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

fn push_tl_string(out: &mut Vec<u8>, text: &[u8]) {
    if text.len() < 254 {
        out.push(text.len() as u8);
    } else {
        out.push(0xfe);
        let len = text.len() as u32;
        out.extend_from_slice(&len.to_le_bytes()[..3]);
    }
    out.extend_from_slice(text);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

struct Options {
    api_id: i32,
    device: Vec<u8>,
    system: Vec<u8>,
    app: Vec<u8>,
    system_lang: Vec<u8>,
    language_pack: Vec<u8>,
    language_code: Vec<u8>,
    tz_offset: i32,
    is_emulator: bool,
    proxy_kind: String,
    proxy_server: Vec<u8>,
    proxy_port: i32,
}

fn have_proxy(options: &Options) -> bool {
    options.proxy_kind == "mtproto" || options.proxy_kind == "web"
}

fn language_pair(options: &Options) -> (Vec<u8>, Vec<u8>) {
    let custom = options.language_code.first() == Some(&b'X');
    if options.language_pack.is_empty() || custom {
        return (Vec::new(), Vec::new());
    }
    let code = if options.language_code.is_empty() {
        b"en".to_vec()
    } else {
        options.language_code.clone()
    };
    (options.language_pack.clone(), code)
}

fn gen_header(options: &Options, is_anonymous: bool) -> Vec<u8> {
    let proxy = !is_anonymous && have_proxy(options);
    let mut flags: i32 = 0;
    if proxy {
        flags |= 1 << 0;
    }
    if !is_anonymous {
        flags |= 1 << 1;
    }
    if options.is_emulator {
        flags |= 1 << 10;
    }

    let mut out = Vec::new();
    out.extend_from_slice(&INVOKE_WITH_LAYER.to_le_bytes());
    out.extend_from_slice(&LAYER.to_le_bytes());
    out.extend_from_slice(&INIT_CONNECTION.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&options.api_id.to_le_bytes());
    if is_anonymous {
        push_tl_string(&mut out, b"n/a");
        push_tl_string(&mut out, b"n/a");
    } else {
        push_tl_string(&mut out, &options.device);
        push_tl_string(&mut out, &options.system);
    }
    push_tl_string(&mut out, &options.app);
    push_tl_string(&mut out, &options.system_lang);
    if is_anonymous {
        push_tl_string(&mut out, b"");
        push_tl_string(&mut out, b"");
    } else {
        let (pack, code) = language_pair(options);
        push_tl_string(&mut out, &pack);
        push_tl_string(&mut out, &code);
    }
    if proxy {
        out.extend_from_slice(&INPUT_CLIENT_PROXY.to_le_bytes());
        push_tl_string(&mut out, &options.proxy_server);
        let port = if options.proxy_kind == "web" { 443 } else { options.proxy_port };
        out.extend_from_slice(&port.to_le_bytes());
    }
    if !is_anonymous {
        out.extend_from_slice(&JSON_OBJECT.to_le_bytes());
        out.extend_from_slice(&VECTOR.to_le_bytes());
        out.extend_from_slice(&1i32.to_le_bytes());
        out.extend_from_slice(&JSON_OBJECT_VALUE.to_le_bytes());
        push_tl_string(&mut out, b"tz_offset");
        out.extend_from_slice(&JSON_NUMBER.to_le_bytes());
        out.extend_from_slice(&(options.tz_offset as f64).to_le_bytes());
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 11 {
        eprintln!("usage: header_probe <api_id> <device> <system> <app> <sys_lang> <lang_pack> <lang_code> <tz_offset> <emulator> <proxy> [server] [port]");
        return ExitCode::from(2);
    }
    let options = Options {
        api_id: args[1].parse().unwrap_or(0),
        device: args[2].as_bytes().to_vec(),
        system: args[3].as_bytes().to_vec(),
        app: args[4].as_bytes().to_vec(),
        system_lang: args[5].as_bytes().to_vec(),
        language_pack: args[6].as_bytes().to_vec(),
        language_code: args[7].as_bytes().to_vec(),
        tz_offset: args[8].parse().unwrap_or(0),
        is_emulator: args[9] != "0",
        proxy_kind: args[10].clone(),
        proxy_server: args.get(11).cloned().unwrap_or_else(|| "127.0.0.1".to_string()).into_bytes(),
        proxy_port: args.get(12).and_then(|port| port.parse().ok()).unwrap_or(1080),
    };
    println!("default={}", hex_encode(&gen_header(&options, false)));
    println!("anonymous={}", hex_encode(&gen_header(&options, true)));
    ExitCode::SUCCESS
}