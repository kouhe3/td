//! td::mtproto::ProxySecret on the Rust side: MTProxy secret validation and
//! encoding, byte-compatible with TDLib's own class.
//!
//! Oracle: tools/fixtures/proxy_secret_probe.cpp (same CLI, links the built
//! libtdmtproto.a). Accepted secrets print raw / proxy / encoded / padding / tls
//! (plus domain for TLS secrets); rejections print code and message.
//!
//! ```text
//! proxy_secret_probe link <text> [--truncate]
//! proxy_secret_probe binary <hex> [--truncate]
//! proxy_secret_probe encoded <hex>
//! proxy_secret_probe derived <hex>
//! ```

use std::env;
use std::process::ExitCode;

const MAX_DOMAIN_LENGTH: usize = 182;
const URL_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const STD_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        out.push((high << 4) | low);
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
            let digit = value(alphabet, body[index + position])?;
            value_c |= (digit as u32) << ((3 - position) * 6);
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

fn from_binary(raw: &[u8], truncate: bool) -> Result<Vec<u8>, (i32, &'static str)> {
    let mut raw = raw;
    if raw.len() > 17 + MAX_DOMAIN_LENGTH {
        if truncate {
            raw = &raw[..17 + MAX_DOMAIN_LENGTH];
        } else {
            return Err((400, "Too long secret"));
        }
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

fn from_link(text: &[u8], truncate: bool) -> Result<Vec<u8>, (i32, &'static str)> {
    let decoded = hex_decode(text)
        .or_else(|| base64_decode_impl(true, text))
        .or_else(|| base64_decode_impl(false, text))
        .ok_or((400, "Wrong proxy secret"))?;
    from_binary(&decoded, truncate)
}

fn emulate_tls(secret: &[u8]) -> bool {
    secret.len() >= 17 && secret[0] == 0xee
}

fn use_random_padding(secret: &[u8]) -> bool {
    secret.len() >= 17
}

fn proxy_secret(secret: &[u8]) -> &[u8] {
    if secret.len() >= 17 {
        &secret[1..17]
    } else {
        secret
    }
}

fn encoded_secret(secret: &[u8]) -> Vec<u8> {
    if emulate_tls(secret) {
        base64_encode_impl(true, secret)
    } else {
        hex_encode(secret).into_bytes()
    }
}

fn print_hex(name: &str, bytes: &[u8]) {
    print!("{}={} ", name, hex_encode(bytes));
}

fn print_secret(secret: &[u8]) {
    print_hex("raw", secret);
    print_hex("proxy", proxy_secret(secret));
    print_hex("encoded", &encoded_secret(secret));
    print!(
        "padding={} tls={}",
        use_random_padding(secret) as u8,
        emulate_tls(secret) as u8
    );
    if emulate_tls(secret) {
        print!(" ");
        print_hex("domain", &secret[17..]);
    }
    println!();
}

fn unhex(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index + 1 < bytes.len() {
        let high = hex_digit(bytes[index]).unwrap_or(0);
        let low = hex_digit(bytes[index + 1]).unwrap_or(0);
        out.push((high << 4) | low);
        index += 2;
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: proxy_secret_probe <link|binary|encoded|derived> <value> [--truncate]");
        return ExitCode::from(2);
    }
    let mode = args[1].as_str();
    let truncate = args.get(3).map(|flag| flag == "--truncate").unwrap_or(false);

    if mode == "link" {
        match from_link(args[2].as_bytes(), truncate) {
            Ok(secret) => {
                print!("status=ok ");
                print_secret(&secret);
            }
            Err((code, message)) => {
                println!("status=err code={code} message={message}");
            }
        }
        return ExitCode::SUCCESS;
    }
    if mode == "binary" {
        let bytes = unhex(&args[2]);
        match from_binary(&bytes, truncate) {
            Ok(secret) => {
                print!("status=ok ");
                print_secret(&secret);
            }
            Err((code, message)) => {
                println!("status=err code={code} message={message}");
            }
        }
        return ExitCode::SUCCESS;
    }
    if mode == "encoded" || mode == "derived" {
        let secret = unhex(&args[2]);
        print!("status=ok ");
        print_secret(&secret);
        return ExitCode::SUCCESS;
    }
    eprintln!("unknown mode {mode}");
    ExitCode::from(2)
}