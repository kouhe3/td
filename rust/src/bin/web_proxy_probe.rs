//! Web-proxy helpers from td/telegram/net/Proxy.cpp on the Rust side.
//!
//! Oracle: tools/fixtures/web_proxy_probe.cpp (same CLI).
//!
//! ```text
//! web_proxy_probe secret <encoded_secret>
//! web_proxy_probe link <link_secret> <has_path:0|1>
//! web_proxy_probe proxy <server> <path> <secret_hex>
//! ```
//!
//! Rules: get_web_proxy_secret rejects fake-TLS secrets; get_web_proxy_link_secret
//! accepts the canonical unpadded base64url of 0x70 || secret (re-encoding must
//! match), otherwise falls back to the plain secret or reports "Wrong proxy
//! secret" when a path is present; a web proxy with a path encodes its link
//! secret with the 0x70 marker and derives its bridge capability from
//! HMAC-SHA256(raw_secret, "tdesktop-web-proxy-bridge-v{1,2}\n" + server[+path]).

use std::env;
use std::process::ExitCode;

use hmac::{Hmac, Mac};
use sha2::Sha256;

const MAX_DOMAIN_LENGTH: usize = 182;
const URL_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const STD_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const MARKER: u8 = 0x70;

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hex_decode(text: &[u8]) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.chunks(2) {
        out.push((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?);
    }
    Some(out)
}

fn value(table: &[u8; 64], byte: u8) -> Option<u8> {
    table.iter().position(|&c| c == byte).map(|index| index as u8)
}

fn base64_drop_padding(is_url: bool, text: &[u8]) -> Option<&[u8]> {
    let mut body = text;
    let mut padding = 0;
    while let Some((&b'=', rest)) = body.split_last() {
        body = rest;
        padding += 1;
    }
    if padding >= 3 {
        return None;
    }
    if (!is_url || padding > 0) && ((body.len() + padding) & 3) != 0 {
        return None;
    }
    if is_url && (body.len() & 3) == 1 {
        return None;
    }
    Some(body)
}

fn base64_decode_impl(is_url: bool, text: &[u8]) -> Option<Vec<u8>> {
    let alphabet = if is_url { URL_ALPHABET } else { STD_ALPHABET };
    let body = base64_drop_padding(is_url, text)?;
    let mut out = Vec::with_capacity(body.len() / 4 * 3 + ((body.len() & 3) + 1) / 2);
    let mut index = 0;
    while index < body.len() {
        let left = std::cmp::min(body.len() - index, 4);
        let mut value_c: u32 = 0;
        for position in 0..left {
            value_c |= (value(alphabet, body[index + position])? as u32) << ((3 - position) * 6);
        }
        index += left;
        out.push((value_c >> 16) as u8);
        if left == 2 {
            if (value_c & 0xffff) != 0 {
                return None;
            }
        } else {
            out.push((value_c >> 8) as u8);
            if left == 3 {
                if (value_c & 0xff) != 0 {
                    return None;
                }
            } else {
                out.push(value_c as u8);
            }
        }
    }
    Some(out)
}

fn base64_encode_impl(is_url: bool, input: &[u8]) -> Vec<u8> {
    let alphabet = if is_url { URL_ALPHABET } else { STD_ALPHABET };
    let mut out = Vec::with_capacity((input.len() + 2) / 3 * 4);
    let mut index = 0;
    while index < input.len() {
        let left = std::cmp::min(input.len() - index, 3);
        let mut c: u32 = (input[index] as u32) << 16;
        index += 1;
        out.push(alphabet[(c >> 18) as usize]);
        if left != 1 {
            c |= (input[index] as u32) << 8;
            index += 1;
        }
        out.push(alphabet[((c >> 12) & 63) as usize]);
        if left == 3 {
            c |= input[index] as u32;
            index += 1;
        }
        if left != 1 {
            out.push(alphabet[((c >> 6) & 63) as usize]);
        } else if !is_url {
            out.push(b'=');
        }
        if left == 3 {
            out.push(alphabet[(c & 63) as usize]);
        } else if !is_url {
            out.push(b'=');
        }
    }
    out
}

fn from_binary(raw: &[u8]) -> Result<Vec<u8>, (i32, &'static str)> {
    if raw.len() > 17 + MAX_DOMAIN_LENGTH {
        return Err((400, "Too long secret"));
    }
    let accepted = raw.len() == 16
        || (raw.len() == 17 && raw[0] == 0xdd)
        || (raw.len() >= 18 && raw[0] == 0xee);
    if accepted {
        return Ok(raw.to_vec());
    }
    if raw.len() < 16 {
        return Err((400, "Wrong proxy secret"));
    }
    Err((400, "Unsupported proxy secret"))
}

fn from_link(text: &[u8]) -> Result<Vec<u8>, (i32, &'static str)> {
    let decoded = hex_decode(text)
        .or_else(|| base64_decode_impl(true, text))
        .or_else(|| base64_decode_impl(false, text))
        .ok_or((400, "Wrong proxy secret"))?;
    from_binary(&decoded)
}

fn emulate_tls(secret: &[u8]) -> bool {
    secret.len() >= 17 && secret[0] == 0xee
}

fn encoded_secret(secret: &[u8]) -> Vec<u8> {
    if emulate_tls(secret) {
        base64_encode_impl(true, secret)
    } else {
        hex_encode(secret).into_bytes()
    }
}

fn web_proxy_secret(link: &[u8]) -> Result<Vec<u8>, (i32, &'static str)> {
    let secret = from_link(link)?;
    if emulate_tls(&secret) {
        return Err((400, "Web proxies don't support fake TLS secrets"));
    }
    Ok(secret)
}

fn web_proxy_link_secret(link: &[u8], has_path: bool) -> Result<Vec<u8>, (i32, &'static str)> {
    if let Some(marked) = base64_decode_impl(true, link) {
        if marked.len() >= 17 && marked[0] == MARKER && base64_encode_impl(true, &marked) == link {
            if let Ok(secret) = from_binary(&marked[1..]) {
                if !emulate_tls(&secret) {
                    return Ok(secret);
                }
            }
        }
    }
    if has_path {
        return Err((400, "Wrong proxy secret"));
    }
    web_proxy_secret(link)
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn print_hex(name: &str, bytes: &[u8]) {
    print!("{}={}", name, hex_encode(bytes));
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: web_proxy_probe secret|link|proxy ...");
        return ExitCode::from(2);
    }
    match args[1].as_str() {
        "secret" => match web_proxy_secret(args[2].as_bytes()) {
            Ok(secret) => {
                print!("status=ok ");
                print_hex("raw", &secret);
                println!();
            }
            Err((code, message)) => println!("status=err code={code} message={message}"),
        },
        "link" if args.len() >= 4 => {
            let has_path = args[3] != "0";
            match web_proxy_link_secret(args[2].as_bytes(), has_path) {
                Ok(secret) => {
                    print!("status=ok ");
                    print_hex("raw", &secret);
                    println!();
                }
                Err((code, message)) => println!("status=err code={code} message={message}"),
            }
        }
        "proxy" if args.len() >= 5 => {
            let server = args[2].as_bytes();
            let path = args[3].as_bytes();
            let secret = match hex_decode(args[4].as_bytes()) {
                Some(secret) => secret,
                None => Vec::new(),
            };
            let address = if path.is_empty() {
                server.to_vec()
            } else {
                [server, b"/", path].concat()
            };
            let link = if path.is_empty() {
                encoded_secret(&secret)
            } else {
                base64_encode_impl(true, &[&[MARKER], secret.as_slice()].concat())
            };
            let context = if path.is_empty() {
                [b"tdesktop-web-proxy-bridge-v1\n".as_slice(), server].concat()
            } else {
                [b"tdesktop-web-proxy-bridge-v2\n".as_slice(), server, b"\n", path].concat()
            };
            let bridge = base64_encode_impl(true, &hmac_sha256(&secret, &context));
            print!("status=ok ");
            print_hex("address", &address);
            print!(" ");
            print_hex("link", &link);
            print!(" ");
            print_hex("bridge", &bridge);
            println!();
        }
        other => {
            eprintln!("bad arguments for mode {other}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}