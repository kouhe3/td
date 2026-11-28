// (Unofficial) Rust side of the `secure_storage_file` lane; the oracle is
// `tools/fixtures/secure_storage_file_probe.cpp` over the file-level half of
// `td/telegram/SecureStorage.cpp`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin secure_storage_file_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/secure_storage_file_probe.exe <script.txt> <workdir>
//
// What is reproduced (the primitives are F244; the file layer is what this lane
// judges):
//   * `gen_random_prefix` (`((32 + 15 + n) & -16) - n`, first byte = length,
//     random content) — the length and the first byte are judged, the content is
//     not;
//   * `calc_aes_cbc_state_sha512(secret || value_hash)`: key = bytes 0..31 of
//     the digest, iv = bytes 32..47, and CBC without extra padding, so the
//     ciphertext is exactly as long as `prefix || data`;
//   * `encrypt_file`: `FileFd::open` order (source, then destination with
//     truncate/create, then the size), the 128 KiB `data_view_for_each` step,
//     the `Encryptor` state machine and the returned `ValueHash`;
//   * `decrypt_file`: the same step over the ciphertext, the `Decryptor` state
//     machine (prefix skip, `Too small random prefix`, `No data was given`,
//     `Part size must be divisible by 16`), the destination write happening
//     *before* the final hash check, and `Hash mismatch`;
//   * the probe's own `mkenc` (a deterministic ciphertext for fixed vectors) and
//     `tamper` (one-bit flip in the last ciphertext block), plus md5 of the
//     decrypted file, because the narration is `len` + `md5`.
//
// md5 is written out here (RFC 1321): the crate list in Cargo.toml has no md5 and
// this lane may not edit Cargo.toml; the tables are the published constants.

use std::io::Write;

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes256;
use sha2::{Digest, Sha256, Sha512};

#[allow(dead_code)]
#[path = "../script_tokens.rs"]
mod script_tokens;

use script_tokens::{split_tokens, trim_line};

// ---------------------------------------------------------------- primitives

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn sha512(data: &[u8]) -> [u8; 64] {
    let mut hasher = Sha512::new();
    hasher.update(data);
    hasher.finalize().into()
}

struct AesCbcState {
    cipher: Aes256,
    iv: [u8; 16],
}

impl AesCbcState {
    fn new(key: &[u8], iv: &[u8]) -> AesCbcState {
        assert_eq!(key.len(), 32, "AesCbcState key");
        assert_eq!(iv.len(), 16, "AesCbcState iv");
        AesCbcState { cipher: Aes256::new(key.into()), iv: iv.try_into().unwrap() }
    }

    /// CBC with no padding: every call is a whole number of blocks.
    fn encrypt(&mut self, data: &mut [u8]) {
        assert!(data.len() % 16 == 0, "AesCbcState::encrypt");
        for chunk in data.chunks_mut(16) {
            for i in 0..16 {
                chunk[i] ^= self.iv[i];
            }
            self.cipher.encrypt_block(chunk.into());
            self.iv.copy_from_slice(chunk);
        }
    }

    fn decrypt(&mut self, data: &mut [u8]) {
        assert!(data.len() % 16 == 0, "AesCbcState::decrypt");
        for chunk in data.chunks_mut(16) {
            let cipher_text: [u8; 16] = chunk.try_into().unwrap();
            self.cipher.decrypt_block(chunk.into());
            for i in 0..16 {
                chunk[i] ^= self.iv[i];
            }
            self.iv = cipher_text;
        }
    }
}

fn calc_aes_cbc_state_sha512(seed: &[u8]) -> AesCbcState {
    let hash = sha512(seed);
    AesCbcState::new(&hash[0..32], &hash[32..48])
}

// ------------------------------------------------------------------ hashing

struct ValueHash([u8; 32]);

impl ValueHash {
    fn create(data: &[u8]) -> Result<ValueHash, String> {
        if data.len() != 32 {
            return Err(format!("Wrong hash size {}", data.len()));
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(data);
        Ok(ValueHash(hash))
    }

    fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

fn calc_value_hash_slice(data: &[u8]) -> ValueHash {
    ValueHash(sha256(data))
}

// ------------------------------------------------------------------- secrets

fn secret_checksum(secret: &[u8]) -> u8 {
    let mut sum: u32 = 0;
    for byte in secret {
        sum += u32::from(*byte);
    }
    ((255 + 239 - sum % 255) % 255) as u8
}

struct Secret {
    bytes: [u8; 32],
}

impl Secret {
    fn create(secret: &[u8]) -> Result<Secret, String> {
        if secret.len() != 32 {
            return Err("Wrong secret size".to_string());
        }
        let checksum = secret_checksum(secret);
        if checksum != 0 {
            return Err(format!("Wrong checksum {}", checksum));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(secret);
        Ok(Secret { bytes })
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

// ------------------------------------------------------------- random prefix

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptGenRandom(algorithm: *mut u8, buffer: *mut u8, len: u32, flags: u32) -> i32;
}

fn fill_random(buffer: &mut [u8]) {
    // BCRYPT_USE_SYSTEM_PREFERRED_RNG == 2. The content of the prefix is not
    // judged; its length and its first byte are.
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), buffer.as_mut_ptr(), buffer.len() as u32, 2) };
    assert_eq!(status, 0, "BCryptGenRandom failed");
}

fn gen_random_prefix(data_size: i64) -> Vec<u8> {
    let size = ((32 + 15 + data_size) & -16) - data_size;
    let mut prefix = vec![0u8; size as usize];
    fill_random(&mut prefix);
    prefix[0] = size as u8;
    assert_eq!((prefix.len() as i64 + data_size) % 16, 0, "gen_random_prefix alignment");
    prefix
}

// ----------------------------------------------------------------------- md5

const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15,
    21,
];

const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501, 0x698098d8,
    0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340,
    0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87,
    0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
    0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039,
    0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92,
    0xffeff47d, 0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
    0xeb86d391,
];

fn md5(data: &[u8]) -> [u8; 16] {
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_le_bytes());

    let mut state: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    for block in message.chunks(64) {
        let mut words = [0u32; 16];
        for (index, word) in words.iter_mut().enumerate() {
            let mut bytes = [0u8; 4];
            bytes.copy_from_slice(&block[index * 4..index * 4 + 4]);
            *word = u32::from_le_bytes(bytes);
        }
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f
                .wrapping_add(a)
                .wrapping_add(MD5_K[i])
                .wrapping_add(words[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(MD5_S[i]));
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }

    let mut out = [0u8; 16];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

// ------------------------------------------------------------- state machines

struct Decryptor {
    state: AesCbcState,
    hasher: Sha256,
    skipped_prefix: bool,
    to_skip: usize,
}

impl Decryptor {
    fn new(state: AesCbcState) -> Decryptor {
        Decryptor { state, hasher: Sha256::new(), skipped_prefix: false, to_skip: 0 }
    }

    fn append(&mut self, mut data: Vec<u8>) -> Result<Vec<u8>, String> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        if data.len() % 16 != 0 {
            return Err("Part size must be divisible by 16".to_string());
        }
        self.state.decrypt(&mut data);
        self.hasher.update(&data);
        if !self.skipped_prefix {
            self.to_skip = usize::from(data[0]);
            let to_skip = self.to_skip.min(data.len());
            if self.to_skip > data.len() {
                // Kept so that `finish` fails the prefix check.
                self.to_skip = 0;
            }
            self.skipped_prefix = true;
            return Ok(data[to_skip..].to_vec());
        }
        Ok(data)
    }

    fn finish(self) -> Result<ValueHash, String> {
        if !self.skipped_prefix {
            return Err("No data was given".to_string());
        }
        if self.to_skip < 32 {
            return Err("Too small random prefix".to_string());
        }
        Ok(ValueHash(self.hasher.finalize().into()))
    }
}

// ---------------------------------------------------------------- file layer

const STEP: i64 = 128 << 10;

fn open_read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|_| format!("File \"{}\" can't be opened for reading", path))
}

fn open_write(path: &str) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path).map_err(|_| {
        format!("File \"{}\" can't be opened/created for writing with truncation", path)
    })
}

fn size_tag(path: &str) -> String {
    match std::fs::metadata(path) {
        Ok(info) => info.len().to_string(),
        Err(_) => "-".to_string(),
    }
}

fn encrypt_file(secret: &Secret, src: &str, dest: &str) -> Result<ValueHash, String> {
    let data = open_read(src)?;
    let mut out = open_write(dest)?;
    let size = data.len() as i64;

    let mut full = gen_random_prefix(size);
    full.extend_from_slice(&data);

    let hash = calc_value_hash_slice(&full);
    let mut seed = Vec::with_capacity(64);
    seed.extend_from_slice(secret.as_slice());
    seed.extend_from_slice(hash.as_slice());
    let mut state = calc_aes_cbc_state_sha512(&seed);

    let mut offset: i64 = 0;
    while offset < full.len() as i64 {
        let part = STEP.min(full.len() as i64 - offset) as usize;
        let chunk = &mut full[offset as usize..offset as usize + part];
        state.encrypt(chunk);
        out.write_all(chunk).map_err(|error| error.to_string())?;
        offset += part as i64;
    }
    Ok(hash)
}

fn decrypt_file(secret: &Secret, hash: &ValueHash, src: &str, dest: &str) -> Result<(), String> {
    let data = open_read(src)?;
    let mut out = open_write(dest)?;
    let size = data.len() as i64;

    let mut seed = Vec::with_capacity(64);
    seed.extend_from_slice(secret.as_slice());
    seed.extend_from_slice(hash.as_slice());
    let mut decryptor = Decryptor::new(calc_aes_cbc_state_sha512(&seed));

    let mut offset: i64 = 0;
    while offset < size {
        let part = STEP.min(size - offset) as usize;
        let decrypted = decryptor.append(data[offset as usize..offset as usize + part].to_vec())?;
        out.write_all(&decrypted).map_err(|error| error.to_string())?;
        offset += part as i64;
    }

    let stored_hash = decryptor.finish()?;
    if stored_hash.as_slice() != hash.as_slice() {
        return Err("Hash mismatch".to_string());
    }
    Ok(())
}

// -------------------------------------------------------------------- script

fn parse_bytes(token: &[u8]) -> Vec<u8> {
    if token == b"-" {
        return Vec::new();
    }
    let text = std::str::from_utf8(token).unwrap();
    assert!(text.len() % 2 == 0, "bad hex");
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).expect("bad hex digit"))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn token_text(token: &[u8]) -> String {
    std::str::from_utf8(token).expect("token").to_string()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn prefix_size(data_size: i64) -> i64 {
    ((32 + 15 + data_size) & -16) - data_size
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: secure_storage_file_probe <script.txt> <workdir>");
        std::process::exit(2);
    }
    let secret_hex = "00000000000000000000000000000000000000000000000000000000000000ef";
    let secret = Secret::create(&parse_bytes(secret_hex.as_bytes())).expect("fixed secret");

    // The script path is relative to the caller's directory, every path inside
    // the script is relative to <workdir>: read the script, then chdir.
    let script = std::fs::read(&args[1]).expect("cannot read script");
    std::env::set_current_dir(&args[2]).expect("cannot chdir into workdir");

    let mut out = std::io::stdout();
    for raw_line in script.split(|c| *c == b'\n') {
        let line = trim_line(raw_line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens = split_tokens(line);
        match tokens[0] {
            b"put" => {
                let file = token_text(tokens[1]);
                let data = parse_bytes(tokens[2]);
                std::fs::write(&file, &data).expect("put");
                let _ = writeln!(out, "put {} len={}", file, data.len());
            }
            b"enc" => {
                let name = token_text(tokens[1]);
                let src = if tokens.len() > 2 { token_text(tokens[2]) } else { format!("{}.in", name) };
                let dest = if tokens.len() > 3 { token_text(tokens[3]) } else { format!("{}.enc", name) };
                match encrypt_file(&secret, &src, &dest) {
                    Err(error) => {
                        let _ = writeln!(out, "enc {} err={} dst={}", name, error, size_tag(&dest));
                    }
                    Ok(hash) => {
                        std::fs::write(format!("{}.hash", name), hash.as_slice()).expect("hash sidecar");
                        let cipher = std::fs::read(&dest).expect("ciphertext");
                        let payload = std::fs::read(&src).expect("payload");
                        let mut plain = cipher.clone();
                        let mut seed = Vec::with_capacity(64);
                        seed.extend_from_slice(secret.as_slice());
                        seed.extend_from_slice(hash.as_slice());
                        calc_aes_cbc_state_sha512(&seed).decrypt(&mut plain);
                        let stored = sha256(&plain) == *hash.as_slice();
                        let contains_field = if payload.len() < 16 {
                            "-".to_string()
                        } else if contains(&cipher, &payload) {
                            "1".to_string()
                        } else {
                            "0".to_string()
                        };
                        let _ = writeln!(
                            out,
                            "enc {} src={} dst={} aligned={} prefix={} stored={} contains={}",
                            name,
                            payload.len(),
                            cipher.len(),
                            usize::from(cipher.len() % 16 == 0),
                            plain.first().map(|byte| *byte as i32).unwrap_or(-1),
                            usize::from(stored),
                            contains_field
                        );
                    }
                }
            }
            b"encdec" => {
                let name = token_text(tokens[1]);
                let src = format!("{}.in", name);
                match encrypt_file(&secret, &src, &format!("{}.enc", name)) {
                    Err(error) => {
                        let _ = writeln!(out, "encdec {} err=enc:{}", name, error);
                    }
                    Ok(hash) => {
                        let dec = decrypt_file(&secret, &hash, &format!("{}.enc", name), &format!("{}.dec", name));
                        match dec {
                            Err(error) => {
                                let _ = writeln!(out, "encdec {} err=dec:{}", name, error);
                            }
                            Ok(()) => {
                                let back = std::fs::read(format!("{}.dec", name)).expect("decrypted");
                                let payload = std::fs::read(&src).expect("payload");
                                let _ = writeln!(
                                    out,
                                    "encdec {} same={} len={} md5={}",
                                    name,
                                    usize::from(back == payload),
                                    back.len(),
                                    hex(&md5(&back))
                                );
                            }
                        }
                    }
                }
            }
            b"dec" => {
                let name = token_text(tokens[1]);
                let src = if tokens.len() > 3 { token_text(tokens[3]) } else { format!("{}.enc", name) };
                let dest = if tokens.len() > 4 { token_text(tokens[4]) } else { format!("{}.dec", name) };
                match ValueHash::create(&parse_bytes(tokens[2])) {
                    Err(error) => {
                        let _ = writeln!(out, "dec {} err={} leftover={}", name, error, size_tag(&dest));
                    }
                    Ok(hash) => match decrypt_file(&secret, &hash, &src, &dest) {
                        Err(error) => {
                            let _ = writeln!(out, "dec {} err={} leftover={}", name, error, size_tag(&dest));
                        }
                        Ok(()) => {
                            let back = std::fs::read(&dest).expect("decrypted");
                            let _ = writeln!(out, "dec {} ok len={} md5={}", name, back.len(), hex(&md5(&back)));
                        }
                    },
                }
            }
            b"mkenc" => {
                let name = token_text(tokens[1]);
                let payload = parse_bytes(tokens[2]);
                let prefix = prefix_size(payload.len() as i64);
                let mut plain = vec![0u8; prefix as usize];
                for i in 1..plain.len() {
                    plain[i] = ((i * 37 + 11) & 0xff) as u8;
                }
                plain[0] = prefix as u8;
                plain.extend_from_slice(&payload);

                let hash = calc_value_hash_slice(&plain);
                let mut seed = Vec::with_capacity(64);
                seed.extend_from_slice(secret.as_slice());
                seed.extend_from_slice(hash.as_slice());
                let mut cipher = plain;
                calc_aes_cbc_state_sha512(&seed).encrypt(&mut cipher);
                std::fs::write(format!("{}.enc", name), &cipher).expect("crafted ciphertext");
                std::fs::write(format!("{}.hash", name), hash.as_slice()).expect("hash sidecar");
                let _ = writeln!(out, "mkenc {} hash={} size={} prefix={}", name, hex(hash.as_slice()), cipher.len(), prefix);
            }
            b"tamper" => {
                let name = token_text(tokens[1]);
                let mut cipher = std::fs::read(format!("{}.enc", name)).expect("ciphertext");
                if cipher.is_empty() {
                    let _ = writeln!(out, "tamper {} size=0 empty=1", name);
                    continue;
                }
                let last = cipher.len() - 1;
                cipher[last] ^= 1;
                std::fs::write(format!("{}.enc", name), &cipher).expect("tampered ciphertext");
                let _ = writeln!(out, "tamper {} size={}", name, cipher.len());
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}