//! TlsInit's ClientHello on the Rust side.
//!
//! Oracle: tools/fixtures/tls_init_probe.cpp (same CLI, same op tree). The
//! output is not byte-comparable across runs: the 32-byte session id, the two
//! X25519 keys, the 1184-byte ML-KEM blob and the ClientHello random (an HMAC
//! over the whole buffer) are all fresh, and `Permutation` shuffles the
//! extension order. `tools/tls_init_diff.py` parses the bytes and compares the
//! structure instead: extension set, extension lengths, and every byte that is
//! fixed by construction.
//!
//! ```text
//! tls_init_probe <domain> <secret_hex> <unix_time> <runs>
//! ```
//!
//! One `hello len=<n> <hex>` line per run. The op tree is built once per
//! process, like TDLib's `static TlsHello` in `TlsHello::get_default()`.

use std::env;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use num_bigint::BigUint;

const MAX_DOMAIN_LENGTH: usize = 182;
const PADDING_MIN: usize = 513;
const PADDING_TARGET: usize = 517;

#[derive(Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x2545_f491_4f6c_dd1d);
        Self(nanos ^ 0x9e37_79b9_7f4a_7c15)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn fill(&mut self, out: &mut [u8]) {
        for byte in out {
            *byte = self.next() as u8;
        }
    }
    fn below(&mut self, bound: u32) -> u32 {
        (self.next() % u64::from(bound)) as u32
    }
    fn fast(&mut self, low: i32, high: i32) -> i32 {
        low + self.below((high - low + 1) as u32) as i32
    }
}

#[derive(Clone)]
enum Op {
    Str(&'static [u8]),
    Random(usize),
    Zero(usize),
    Domain,
    Grease(usize),
    Key,
    MlKem768Key,
    BeginScope,
    EndScope,
    Permutation(Vec<Vec<Op>>),
    EchPayload(usize),
    Padding,
}

impl Op {
    fn str(bytes: &'static [u8]) -> Op {
        Op::Str(bytes)
    }
}

struct Context {
    grease: Vec<u8>,
    domain: Vec<u8>,
}

impl Context {
    fn new(domain: &str, rng: &mut Rng) -> Self {
        let mut grease = vec![0u8; 7];
        rng.fill(&mut grease);
        for byte in &mut grease {
            *byte = (*byte & 0xf0) + 0x0a;
        }
        let mut index = 1;
        while index < grease.len() {
            if grease[index] == grease[index - 1] {
                grease[index] ^= 0x10;
            }
            index += 2;
        }
        let mut domain_bytes = domain.as_bytes().to_vec();
        domain_bytes.truncate(MAX_DOMAIN_LENGTH);
        Self { grease, domain: domain_bytes }
    }

    fn domain(&self) -> &[u8] {
        &self.domain
    }
}

fn default_hello(rng: &mut Rng) -> Vec<Op> {
    vec![
        Op::str(b"\x16\x03\x01"),
        Op::BeginScope,
        Op::str(b"\x01\x00"),
        Op::BeginScope,
        Op::str(b"\x03\x03"),
        Op::Zero(32),
        Op::str(b"\x20"),
        Op::Random(32),
        Op::str(b"\x00\x20"),
        Op::Grease(0),
        Op::str(
            b"\x13\x01\x13\x02\x13\x03\xc0\x2b\xc0\x2f\xc0\x2c\xc0\x30\xcc\xa9\xcc\xa8\xc0\x13\xc0\x14\x00\x9c\x00\x9d\x00\x2f\x00\x35\x01\x00",
        ),
        Op::BeginScope,
        Op::Grease(2),
        Op::str(b"\x00\x00"),
        Op::Permutation(vec![
            vec![
                Op::str(b"\x00\x00"),
                Op::BeginScope,
                Op::BeginScope,
                Op::str(b"\x00"),
                Op::BeginScope,
                Op::Domain,
                Op::EndScope,
                Op::EndScope,
                Op::EndScope,
            ],
            vec![Op::str(b"\x00\x05\x00\x05\x01\x00\x00\x00\x00")],
            vec![
                Op::str(b"\x00\x0a\x00\x0c\x00\x0a"),
                Op::Grease(4),
                Op::str(b"\x11\xec\x00\x1d\x00\x17\x00\x18"),
            ],
            vec![Op::str(b"\x00\x0b\x00\x02\x01\x00")],
            vec![Op::str(
                b"\x00\x0d\x00\x18\x00\x16\x09\x04\x09\x05\x09\x06\x04\x03\x08\x04\x04\x01\x05\x03\x08\x05\x05\x01\x08\x06\x06\x01",
            )],
            vec![Op::str(b"\x00\x10\x00\x0e\x00\x0c\x02\x68\x32\x08\x68\x74\x74\x70\x2f\x31\x2e\x31")],
            vec![Op::str(b"\x00\x12\x00\x00")],
            vec![Op::str(b"\x00\x17\x00\x00")],
            vec![Op::str(b"\x00\x1b\x00\x03\x02\x00\x02")],
            vec![Op::str(b"\x00\x23\x00\x00")],
            vec![
                Op::str(b"\x00\x2b\x00\x07\x06"),
                Op::Grease(6),
                Op::str(b"\x03\x04\x03\x03"),
            ],
            vec![Op::str(b"\x00\x2d\x00\x02\x01\x01")],
            vec![
                Op::str(b"\x00\x33\x04\xef\x04\xed"),
                Op::Grease(4),
                Op::str(b"\x00\x01\x00\x11\xec\x04\xc0"),
                Op::MlKem768Key,
                Op::Key,
                Op::str(b"\x00\x1d\x00\x20"),
                Op::Key,
            ],
            vec![Op::str(b"\x44\xcd\x00\x05\x00\x03\x02\x68\x32")],
            vec![
                Op::str(b"\xfe\x0d"),
                Op::BeginScope,
                Op::str(b"\x00\x00\x01\x00\x01"),
                Op::Random(1),
                Op::str(b"\x00\x20"),
                Op::Key,
                Op::BeginScope,
                Op::EchPayload(rng.fast(0, 3) as usize * 32 + 144),
                Op::EndScope,
                Op::EndScope,
            ],
            vec![Op::str(b"\xff\x01\x00\x01\x00")],
        ]),
        Op::Grease(3),
        Op::str(b"\x00\x01\x00"),
        Op::Padding,
        Op::EndScope,
        Op::EndScope,
        Op::EndScope,
    ]
}

/// First pass: total length, mirroring TlsHelloCalcLength. Scopes must balance.
fn calc_length(ops: &[Op], context: &Context, size: &mut usize, scopes: &mut Vec<usize>) -> Result<(), String> {
    for op in ops {
        match op {
            Op::Str(data) => *size += data.len(),
            Op::Random(length) => {
                if *length == 0 || *length > 2048 {
                    return Err(format!("Invalid random length {length}"));
                }
                *size += *length;
            }
            Op::Zero(length) => {
                if *length == 0 || *length > 2048 {
                    return Err(format!("Invalid zero length {length}"));
                }
                *size += *length;
            }
            Op::EchPayload(length) => *size += *length,
            Op::Domain => *size += context.domain().len(),
            Op::Grease(seed) => {
                if *seed >= context.grease.len() {
                    return Err(format!("Invalid grease seed {seed}"));
                }
                *size += 2;
            }
            Op::Key => *size += 32,
            Op::MlKem768Key => *size += 1184,
            Op::BeginScope => {
                *size += 2;
                scopes.push(*size);
            }
            Op::EndScope => {
                let begin = scopes.pop().ok_or("Unbalanced scopes")?;
                if *size - begin >= 1 << 14 {
                    return Err("Scope is too big".to_string());
                }
            }
            Op::Permutation(parts) => {
                for part in parts {
                    calc_length(part, context, size, scopes)?;
                }
            }
            Op::Padding => {
                if *size < PADDING_MIN {
                    *size = PADDING_TARGET;
                }
            }
        }
    }
    Ok(())
}

fn double_x(x: &BigUint, modulus: &BigUint) -> BigUint {
    let one = BigUint::from(1u8);
    let y = y2(x, modulus);
    let denominator = (y * BigUint::from(4u8)) % modulus;
    let numerator = {
        let t = (x * x + modulus - &one) % modulus;
        (t.clone() * t) % modulus
    };
    // Fermat: the modulus is prime, so a^(p-2) is the inverse.
    let exponent = modulus - BigUint::from(2u8);
    let inverse = denominator.modpow(&exponent, modulus);
    (numerator * inverse) % modulus
}

fn y2(x: &BigUint, modulus: &BigUint) -> BigUint {
    let coef = BigUint::from(486_662u32);
    let mut y = (x + &coef) % modulus;
    y = (y * x) % modulus;
    y = (y + BigUint::from(1u8)) % modulus;
    (y * x) % modulus
}

fn gen_key(rng: &mut Rng) -> [u8; 32] {
    let modulus = (BigUint::from(1u8) << 255) - BigUint::from(19u8);
    let exponent = (BigUint::from(1u8) << 254) - BigUint::from(10u8);
    loop {
        let mut key = [0u8; 32];
        rng.fill(&mut key);
        key[31] &= 127;
        let x = BigUint::from_bytes_le(&key);
        if y2(&x, &modulus).modpow(&exponent, &modulus) == BigUint::from(1u8) {
            let mut x = x;
            for _ in 0..3 {
                x = double_x(&x, &modulus);
            }
            let mut out = [0u8; 32];
            let bytes = x.to_bytes_le();
            let take = bytes.len().min(32);
            out[..take].copy_from_slice(&bytes[..take]);
            return out;
        }
    }
}

/// Second pass: write the bytes, back-filling every scope length.
fn store(
    ops: &[Op],
    context: &Context,
    dest: &mut Vec<u8>,
    scopes: &mut Vec<usize>,
    rng: &mut Rng,
) -> Result<(), String> {
    for op in ops {
        match op {
            Op::Str(data) => dest.extend_from_slice(data),
            Op::Random(length) => {
                let mut bytes = vec![0u8; *length];
                rng.fill(&mut bytes);
                dest.extend_from_slice(&bytes);
            }
            Op::Zero(length) => dest.extend(std::iter::repeat(0u8).take(*length)),
            Op::Domain => {
                let domain = context.domain().to_vec();
                dest.extend_from_slice(&domain);
            }
            Op::Grease(seed) => {
                let byte = context.grease[*seed];
                dest.push(byte);
                dest.push(byte);
            }
            Op::Key => {
                let key = gen_key(rng);
                dest.extend_from_slice(&key);
            }
            Op::MlKem768Key => {
                for _ in 0..384 {
                    let a = rng.next() % 3329;
                    let b = rng.next() % 3329;
                    dest.push((a & 255) as u8);
                    dest.push(((a >> 8) + ((b & 15) << 4)) as u8);
                    dest.push((b >> 4) as u8);
                }
                let mut tail = [0u8; 32];
                rng.fill(&mut tail);
                dest.extend_from_slice(&tail);
            }
            Op::BeginScope => {
                scopes.push(dest.len());
                dest.extend_from_slice(&[0, 0]);
            }
            Op::EndScope => {
                let begin = scopes.pop().ok_or("Unbalanced scopes")?;
                let size = dest.len() - begin - 2;
                dest[begin] = ((size >> 8) & 0xff) as u8;
                dest[begin + 1] = (size & 0xff) as u8;
            }
            Op::Permutation(parts) => {
                let mut rendered = Vec::with_capacity(parts.len());
                for part in parts {
                    let mut buffer = Vec::new();
                    let mut sub_scopes = Vec::new();
                    store(part, context, &mut buffer, &mut sub_scopes, rng)?;
                    rendered.push(buffer);
                }
                for index in (1..rendered.len()).rev() {
                    let swap = rng.below((index + 1) as u32) as usize;
                    rendered.swap(index, swap);
                }
                for part in rendered {
                    dest.extend_from_slice(&part);
                }
            }
            Op::Padding => {
                let remaining = PADDING_MIN as isize - dest.len() as isize;
                if remaining > 0 {
                    dest.extend_from_slice(&[0x00, 0x15]);
                    dest.extend_from_slice(&[0, 0]);
                    let begin = dest.len() - 2;
                    dest.extend(std::iter::repeat(0u8).take(remaining as usize));
                    let size = dest.len() - begin - 2;
                    dest[begin] = ((size >> 8) & 0xff) as u8;
                    dest[begin + 1] = (size & 0xff) as u8;
                }
            }
            Op::EchPayload(length) => {
                let mut bytes = vec![0u8; *length];
                rng.fill(&mut bytes);
                dest.extend_from_slice(&bytes);
            }
        }
    }
    Ok(())
}

fn hmac_sha256(secret: &[u8], data: &[u8]) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key");
    mac.update(data);
    let out = mac.finalize().into_bytes();
    let mut result = [0u8; 32];
    result.copy_from_slice(&out);
    result
}

fn finish(data: &mut [u8], secret: &[u8], unix_time: i32) {
    // The random field is the HMAC of the buffer with that field still zero.
    let hash = hmac_sha256(secret, data);
    data[11..43].copy_from_slice(&hash);
    let old = i32::from_le_bytes(data[39..43].try_into().expect("four bytes"));
    data[39..43].copy_from_slice(&(old ^ unix_time).to_le_bytes());
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
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: tls_init_probe <domain> <secret_hex> <unix_time> <runs>");
        return ExitCode::from(2);
    }
    let domain = &args[1];
    let secret = unhex(&args[2]);
    if secret.len() != 16 {
        eprintln!("secret must be 16 bytes, got {}", secret.len());
        return ExitCode::from(2);
    }
    let unix_time: i32 = args[3].parse().unwrap_or(0);
    let runs: usize = args[4].parse().unwrap_or(1);

    let mut rng = Rng::new();
    let context = Context::new(domain, &mut rng);
    let ops = default_hello(&mut rng);

    let mut out = String::new();
    for _ in 0..runs {
        let mut length = 0usize;
        let mut scopes = Vec::new();
        if let Err(error) = calc_length(&ops, &context, &mut length, &mut scopes) {
            eprintln!("calc_length: {error}");
            return ExitCode::from(1);
        }
        if !scopes.is_empty() {
            eprintln!("calc_length: unbalanced scopes");
            return ExitCode::from(1);
        }
        let mut data = Vec::with_capacity(length);
        let mut scopes = Vec::new();
        if let Err(error) = store(&ops, &context, &mut data, &mut scopes, &mut rng) {
            eprintln!("store: {error}");
            return ExitCode::from(1);
        }
        if data.len() != length {
            eprintln!("length mismatch: expected {length}, wrote {}", data.len());
            return ExitCode::from(1);
        }
        finish(&mut data, &secret, unix_time);
        out.push_str(&format!("hello len={} ", data.len()));
        for byte in &data {
            out.push_str(&format!("{byte:02x}"));
        }
        out.push('\n');
    }
    print!("{out}");
    ExitCode::SUCCESS
}