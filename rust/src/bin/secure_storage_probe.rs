// (Unofficial) Rust side of the `secure_storage` lane; the oracle is
// `tools/fixtures/secure_storage_probe.cpp` over `td/telegram/SecureStorage.cpp`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin secure_storage_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/secure_storage_probe.exe <script.txt>
//
// What is reproduced (docs/facts.md F244):
//   * `calc_value_hash` (sha256 over a `Slice`, and over a `DataView` in 128 KiB
//     `pread` steps whose chunk log is part of the narration);
//   * `calc_aes_cbc_state_sha512` / `calc_aes_cbc_state_pbkdf2` (key = first 32
//     bytes of the digest, iv = the next 16; observed by encrypting a zero block,
//     because `AesCbcState` has no accessors);
//   * `Secret`: size and `(255 + 239 - sum % 255) % 255` checksum, `get_hash`
//     (first 8 bytes of `sha256(secret)` read as a little-endian `int64`);
//   * `Secret::encrypt` / `EncryptedSecret::decrypt` for both derivations;
//   * `gen_random_prefix` (`((32 + 15 + n) & -16) - n`, first byte = length,
//     random content);
//   * `Encryptor` / `Decryptor` state machines, including the `to_skip_` reset
//     when the prefix byte exceeds the first part;
//   * `encrypt_value` / `decrypt_value`, so the two sides can read each other's
//     ciphertext, plus the `Hash mismatch` message (hex dumps in groups of four
//     reversed bytes, sixteen groups per line).
//
// The C++ has no CBC in `td/utils/crypto.h` beyond `ctr`; the `aes` crate gives
// the block cipher only, so the chaining lives here.

use std::cell::RefCell;
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

fn calc_aes_cbc_state_hash(hash: &[u8]) -> AesCbcState {
    assert_eq!(hash.len(), 64, "calc_aes_cbc_state_hash");
    AesCbcState::new(&hash[0..32], &hash[32..48])
}

fn calc_aes_cbc_state_sha512(seed: &[u8]) -> AesCbcState {
    calc_aes_cbc_state_hash(&sha512(seed))
}

fn calc_aes_cbc_state_pbkdf2(secret: &[u8], salt: &[u8]) -> AesCbcState {
    let mut hash = [0u8; 64];
    pbkdf2::pbkdf2_hmac::<Sha512>(secret, salt, 100000, &mut hash);
    calc_aes_cbc_state_hash(&hash)
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

trait DataView {
    fn size(&self) -> i64;
    fn pread(&self, offset: i64, size: i64) -> Result<Vec<u8>, String>;
}

struct BufferSliceDataView {
    data: Vec<u8>,
    log: RefCell<String>,
}

impl BufferSliceDataView {
    fn new(data: Vec<u8>) -> BufferSliceDataView {
        BufferSliceDataView { data, log: RefCell::new(String::new()) }
    }

    fn log(&self) -> String {
        self.log.borrow().clone()
    }
}

impl DataView for BufferSliceDataView {
    fn size(&self) -> i64 {
        self.data.len() as i64
    }

    fn pread(&self, offset: i64, size: i64) -> Result<Vec<u8>, String> {
        let chunk = format!("{}:{}", offset, size);
        let mut log = self.log.borrow_mut();
        if !log.is_empty() {
            log.push(',');
        }
        log.push_str(&chunk);
        drop(log);
        let end = offset + size;
        if self.size() < end {
            return Err("Not enough data in BufferSlice".to_string());
        }
        Ok(self.data[offset as usize..end as usize].to_vec())
    }
}

fn calc_value_hash_slice(data: &[u8]) -> ValueHash {
    ValueHash(sha256(data))
}

fn calc_value_hash_view(view: &dyn DataView) -> Result<ValueHash, String> {
    let step: i64 = 128 << 10;
    let mut hasher = Sha256::new();
    let mut i: i64 = 0;
    let size = view.size();
    while i < size {
        let chunk = view.pread(i, step.min(size - i))?;
        hasher.update(&chunk);
        i += step;
    }
    Ok(ValueHash(hasher.finalize().into()))
}

fn hex_dump4(bytes: &[u8]) -> String {
    // `format::as_hex_dump<4>`: a leading newline, groups of four bytes printed
    // reversed, sixteen groups per line, a trailing newline after the last group.
    let mut out = String::from("\n");
    let size = bytes.len();
    let first_part = size % 4;
    if first_part != 0 {
        for byte in &bytes[..first_part] {
            out.push_str(&format!("{:02x}", byte));
        }
        out.push('\n');
    }
    let mut index = first_part;
    while index < size {
        let group = &bytes[index..index + 4];
        for byte in group.iter().rev() {
            out.push_str(&format!("{:02x}", byte));
        }
        if ((index / 4) & 15) == 15 || index + 4 >= size {
            out.push('\n');
        } else {
            out.push(' ');
        }
        index += 4;
    }
    out
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
    hash: i64,
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
        let digest = sha256(secret);
        let hash = i64::from_le_bytes(digest[0..8].try_into().unwrap());
        Ok(Secret { bytes, hash })
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    fn get_hash(&self) -> i64 {
        self.hash
    }

    fn encrypt(&self, key: &[u8], salt: &[u8], algorithm: Algorithm) -> [u8; 32] {
        let mut state = derive_state(algorithm, key, salt);
        let mut out = self.bytes;
        state.encrypt(&mut out);
        out
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Algorithm {
    Sha512,
    Pbkdf2,
}

fn derive_state(algorithm: Algorithm, key: &[u8], salt: &[u8]) -> AesCbcState {
    match algorithm {
        Algorithm::Sha512 => {
            let mut seed = Vec::with_capacity(salt.len() * 2 + key.len());
            seed.extend_from_slice(salt);
            seed.extend_from_slice(key);
            seed.extend_from_slice(salt);
            calc_aes_cbc_state_sha512(&seed)
        }
        Algorithm::Pbkdf2 => calc_aes_cbc_state_pbkdf2(key, salt),
    }
}

fn encrypted_secret_create(data: &[u8]) -> Result<[u8; 32], String> {
    if data.len() != 32 {
        return Err("Wrong encrypted secret size".to_string());
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(data);
    Ok(bytes)
}

fn encrypted_secret_decrypt(
    encrypted: &[u8],
    key: &[u8],
    salt: &[u8],
    algorithm: Algorithm,
) -> Result<Secret, String> {
    let mut state = derive_state(algorithm, key, salt);
    let mut out: [u8; 32] = encrypted.try_into().unwrap_or([0u8; 32]);
    state.decrypt(&mut out);
    Secret::create(&out)
}

// ------------------------------------------------------------- random prefix

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptGenRandom(algorithm: *mut u8, buffer: *mut u8, len: u32, flags: u32) -> i32;
}

fn fill_random(buffer: &mut [u8]) {
    // BCRYPT_USE_SYSTEM_PREFERRED_RNG == 2. The content of the prefix is not
    // judged (F244); the structure is.
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

// ------------------------------------------------------------- state machines

struct Encryptor {
    state: AesCbcState,
    current_offset: i64,
}

impl Encryptor {
    fn new(state: AesCbcState) -> Encryptor {
        Encryptor { state, current_offset: 0 }
    }

    fn pread(&mut self, view: &dyn DataView, offset: i64, size: i64) -> Result<Vec<u8>, String> {
        if offset != self.current_offset {
            return Err("Arbitrary offset is not supported".to_string());
        }
        if size % 16 != 0 {
            return Err("Part size must be divisible by 16".to_string());
        }
        let mut part = view.pread(offset, size)?;
        self.state.encrypt(&mut part);
        self.current_offset += size;
        Ok(part)
    }
}

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

struct EncryptedValue {
    data: Vec<u8>,
    hash: ValueHash,
}

fn encrypt_value(secret: &Secret, data: &[u8]) -> EncryptedValue {
    let prefix = gen_random_prefix(data.len() as i64);
    let mut full = prefix;
    full.extend_from_slice(data);

    let hash = calc_value_hash_slice(&full);
    let mut seed = Vec::with_capacity(64);
    seed.extend_from_slice(secret.as_slice());
    seed.extend_from_slice(hash.as_slice());
    let mut encryptor = Encryptor::new(calc_aes_cbc_state_sha512(&seed));
    let size = full.len() as i64;
    let view = BufferSliceDataView::new(full);
    let encrypted = encryptor.pread(&view, 0, size).expect("encrypt_value pread");
    EncryptedValue { data: encrypted, hash }
}

fn decrypt_value(secret: &Secret, hash: &ValueHash, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut seed = Vec::with_capacity(64);
    seed.extend_from_slice(secret.as_slice());
    seed.extend_from_slice(hash.as_slice());
    let mut decryptor = Decryptor::new(calc_aes_cbc_state_sha512(&seed));
    let value = decryptor.append(data.to_vec())?;
    let stored_hash = decryptor.finish()?;
    if stored_hash.as_slice() != hash.as_slice() {
        return Err(format!(
            "Hash mismatch {} {}",
            hex_dump4(stored_hash.as_slice()),
            hex_dump4(hash.as_slice())
        ));
    }
    Ok(value)
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

fn bits_of_block(state: &mut AesCbcState) -> String {
    let mut block = [0u8; 32];
    state.encrypt(&mut block);
    hex(&block)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: secure_storage_probe <script.txt>");
        std::process::exit(2);
    }
    let secret_hex = "00000000000000000000000000000000000000000000000000000000000000ef";
    let secret = Secret::create(&parse_bytes(secret_hex.as_bytes())).expect("fixed secret");

    let script = std::fs::read(&args[1]).expect("cannot read script");
    let mut out = std::io::stdout();
    let mut decryptor: Option<Decryptor> = None;
    let mut encryptor: Option<Encryptor> = None;
    let mut encryptor_view: Option<BufferSliceDataView> = None;

    for raw_line in script.split(|c| *c == b'\n') {
        let line = trim_line(raw_line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens = split_tokens(line);
        match tokens[0] {
            b"vhash" => {
                let data = parse_bytes(tokens[1]);
                let hash = calc_value_hash_slice(&data);
                let _ = writeln!(out, "vhash={}", hex(hash.as_slice()));
            }
            b"dhash" => {
                let view = BufferSliceDataView::new(parse_bytes(tokens[1]));
                match calc_value_hash_view(&view) {
                    Ok(hash) => {
                        let _ = writeln!(out, "dhash={} chunks={}", hex(hash.as_slice()), view.log());
                    }
                    Err(error) => {
                        let _ = writeln!(out, "dhash err={}", error);
                    }
                }
            }
            b"vcreate" => {
                let data = parse_bytes(tokens[1]);
                match ValueHash::create(&data) {
                    Ok(hash) => {
                        let _ = writeln!(out, "vcreate ok={}", hex(hash.as_slice()));
                    }
                    Err(error) => {
                        let _ = writeln!(out, "vcreate err={}", error);
                    }
                }
            }
            b"aes" => {
                let seed = parse_bytes(tokens[1]);
                let mut state = calc_aes_cbc_state_sha512(&seed);
                let _ = writeln!(out, "aes={}", bits_of_block(&mut state));
            }
            b"pbkdf2" => {
                let key = parse_bytes(tokens[1]);
                let salt = parse_bytes(tokens[2]);
                let mut state = calc_aes_cbc_state_pbkdf2(&key, &salt);
                let _ = writeln!(out, "pbkdf2={}", bits_of_block(&mut state));
            }
            b"skey" => {
                let data = parse_bytes(tokens[1]);
                match Secret::create(&data) {
                    Ok(created) => {
                        let _ = writeln!(
                            out,
                            "skey hash={} hex={}",
                            created.get_hash(),
                            hex(created.as_slice())
                        );
                    }
                    Err(error) => {
                        let _ = writeln!(out, "skey err={}", error);
                    }
                }
            }
            b"enc" => {
                let key = parse_bytes(tokens[1]);
                let salt = parse_bytes(tokens[2]);
                let algorithm = if tokens[3] == b"1" { Algorithm::Pbkdf2 } else { Algorithm::Sha512 };
                let encrypted = secret.encrypt(&key, &salt, algorithm);
                let _ = writeln!(out, "enc={}", hex(&encrypted));
            }
            b"dec" => {
                let key = parse_bytes(tokens[2]);
                let salt = parse_bytes(tokens[3]);
                let algorithm = if tokens[4] == b"1" { Algorithm::Pbkdf2 } else { Algorithm::Sha512 };
                match encrypted_secret_create(&parse_bytes(tokens[1])) {
                    Ok(encrypted) => match encrypted_secret_decrypt(&encrypted, &key, &salt, algorithm) {
                        Ok(decrypted) => {
                            let _ = writeln!(
                                out,
                                "dec ok hash={} hex={}",
                                decrypted.get_hash(),
                                hex(decrypted.as_slice())
                            );
                        }
                        Err(error) => {
                            let _ = writeln!(out, "dec err={}", error);
                        }
                    },
                    Err(error) => {
                        let _ = writeln!(out, "dec err={}", error);
                    }
                }
            }
            b"prefix" => {
                let size: i64 = std::str::from_utf8(tokens[1]).unwrap().parse().unwrap();
                let prefix = gen_random_prefix(size);
                let first = if prefix.is_empty() { -1 } else { i32::from(prefix[0]) };
                let _ = writeln!(out, "prefix len={} first={}", prefix.len(), first);
            }
            b"encval" => {
                let data = parse_bytes(tokens[1]);
                let encrypted = encrypt_value(&secret, &data);
                let _ = writeln!(
                    out,
                    "encval hash={} data={}",
                    hex(encrypted.hash.as_slice()),
                    hex(&encrypted.data)
                );
            }
            b"decval" => {
                let hash = ValueHash::create(&parse_bytes(tokens[1])).expect("decval hash");
                let data = parse_bytes(tokens[2]);
                match decrypt_value(&secret, &hash, &data) {
                    Ok(value) => {
                        let _ = writeln!(out, "decval ok len={} hex={}", value.len(), hex(&value));
                    }
                    Err(error) => {
                        let _ = writeln!(out, "decval err={}", error);
                    }
                }
            }
            b"dstate" => {
                let seed = parse_bytes(tokens[1]);
                decryptor = Some(Decryptor::new(calc_aes_cbc_state_sha512(&seed)));
            }
            b"dappend" => {
                let data = parse_bytes(tokens[1]);
                match decryptor.as_mut().expect("dstate first").append(data) {
                    Ok(value) => {
                        let shown = if value.is_empty() { "-".to_string() } else { hex(&value) };
                        let _ = writeln!(out, "dappend ok len={} hex={}", value.len(), shown);
                    }
                    Err(error) => {
                        let _ = writeln!(out, "dappend err={}", error);
                    }
                }
            }
            b"dfinish" => {
                match decryptor.take().expect("dstate first").finish() {
                    Ok(hash) => {
                        let _ = writeln!(out, "dfinish ok={}", hex(hash.as_slice()));
                    }
                    Err(error) => {
                        let _ = writeln!(out, "dfinish err={}", error);
                    }
                }
            }
            b"estate" => {
                let seed = parse_bytes(tokens[1]);
                encryptor_view = Some(BufferSliceDataView::new(parse_bytes(tokens[2])));
                encryptor = Some(Encryptor::new(calc_aes_cbc_state_sha512(&seed)));
            }
            b"epread" => {
                let offset: i64 = std::str::from_utf8(tokens[1]).unwrap().parse().unwrap();
                let size: i64 = std::str::from_utf8(tokens[2]).unwrap().parse().unwrap();
                let view = encryptor_view.as_ref().expect("estate first");
                let result = encryptor.as_mut().expect("estate first").pread(view, offset, size);
                match result {
                    Ok(value) => {
                        let shown = if value.is_empty() { "-".to_string() } else { hex(&value) };
                        let _ = writeln!(
                            out,
                            "epread ok len={} hex={} chunks={}",
                            value.len(),
                            shown,
                            view.log()
                        );
                    }
                    Err(error) => {
                        let _ = writeln!(out, "epread err={}", error);
                    }
                }
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}