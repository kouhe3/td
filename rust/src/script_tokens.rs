//! Script token helpers shared by the probe bins.
//!
//! `<name>` is plain text or `hex-<bytes>` (so corpora can feed control
//! characters and non-ASCII); `-` is the empty string.

pub fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

pub fn parse_name(token: &[u8]) -> Vec<u8> {
    if token == b"-" {
        return Vec::new();
    }
    if token.len() > 4 && &token[..4] == b"hex-" {
        let hex = &token[4..];
        assert!(hex.len() % 2 == 0, "bad hex");
        let mut out = Vec::with_capacity(hex.len() / 2);
        for pair in hex.chunks(2) {
            let hi = hex_digit(pair[0]).expect("bad hex digit");
            let lo = hex_digit(pair[1]).expect("bad hex digit");
            out.push((hi << 4) | lo);
        }
        return out;
    }
    token.to_vec()
}

pub fn parse_flag(token: &[u8]) -> bool {
    token == b"1"
}

pub fn split<'a>(line: &'a [u8], sep: u8) -> Vec<&'a [u8]> {
    if line.is_empty() {
        return vec![];
    }
    let mut out: Vec<&[u8]> = line.split(|c| *c == sep).collect();
    if line.last() == Some(&sep) {
        out.pop();
    }
    out
}

pub fn parse_csv(token: &[u8]) -> Vec<Vec<u8>> {
    if token == b"-" {
        return Vec::new();
    }
    split(token, b',').into_iter().map(parse_name).collect()
}

/// Whitespace around a script line, leaving the payload alone.
pub fn trim_line(mut line: &[u8]) -> &[u8] {
    while let [rest @ .., last] = line {
        if *last == b'\r' || *last == b' ' || *last == b'\t' {
            line = rest;
        } else {
            break;
        }
    }
    while let [first, rest @ ..] = line {
        if *first == b' ' || *first == b'\t' {
            line = rest;
        } else {
            break;
        }
    }
    line
}

/// Split on single spaces like the C++ probes' `split_tokens`.
pub fn split_tokens(line: &[u8]) -> Vec<&[u8]> {
    let mut tokens = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        while rest.first() == Some(&b' ') {
            rest = &rest[1..];
        }
        if rest.is_empty() {
            break;
        }
        let end = rest.iter().position(|c| *c == b' ').unwrap_or(rest.len());
        tokens.push(&rest[..end]);
        rest = &rest[end..];
    }
    tokens
}
