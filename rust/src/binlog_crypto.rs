//! The AES-256-CTR layer of the binlog: the plaintext `type = -3` header event,
//! the key derivation and the keystream.
//!
//! Shared by `binlog_write_probe` and `tqueue_binlog_probe` (both include it
//! with `#[path = "../binlog_crypto.rs"]`, so it can only use `binlog_format`
//! items that both bins declare).
//!
//! What stays per-bin: when a file gets regenerated (reindex, `change_key`) and
//! how the frames are buffered. This module only knows how to make a header,
//! verify a key against it, and run the keystream at a byte offset.

use aes::Aes256;
use ctr::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::binlog_format::{store_tl_string, take_tl_string, KEY_CHECK};

/// `detail::AesCtrEncryptionEvent::EVENT_TYPE`; always the first frame, id 0.
pub const SERVICE_AES_CTR_ENCRYPTION: i32 = -3;
/// `detail::AesCtrEncryptionEvent` for a raw key (`kdf_fast_iteration_count`)
pub const KDF_ITERATIONS: u32 = 2;
pub const SALT_SIZE: usize = 32;
pub const IV_SIZE: usize = 16;
pub const KEY_HASH_SIZE: usize = 32;
/// `BEGIN_STORE_FLAGS`/`END_STORE_FLAGS` write one zeroed `uint32` first
pub const HEADER_FLAGS_SIZE: usize = 4;
const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;

type Aes256Ctr = ctr::Ctr128BE<Aes256>;

#[link(name = "bcrypt")]
extern "system" {
    /// The RNG `Random::secure_bytes` ends up in on Windows.
    fn BCryptGenRandom(algorithm: *mut core::ffi::c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
}

pub fn secure_bytes(len: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; len];
    let status = unsafe {
        BCryptGenRandom(core::ptr::null_mut(), out.as_mut_ptr(), len as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
    };
    if status != 0 {
        return Err(format!("BCryptGenRandom failed: 0x{status:08x}"));
    }
    Ok(out)
}

/// `PBKDF2-HMAC-SHA256(db_key, key_salt, kdf_fast_iteration_count, 32)`.
pub fn derive_key(key: &[u8], salt: &[u8]) -> [u8; 32] {
    let mut derived = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(key, salt, KDF_ITERATIONS, &mut derived);
    derived
}

/// `AesCtrEncryptionEvent::generate_hash` (HMAC-SHA256 under a literal message).
pub fn key_hash(key: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256>>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(KEY_CHECK);
    mac.finalize().into_bytes().into()
}

/// XOR `data` with the AES-256-CTR keystream at byte position `offset`; CTR is
/// its own inverse, so this both encrypts and decrypts.
pub fn apply_keystream(data: &mut [u8], key: &[u8; 32], iv: &[u8], offset: usize) -> Result<(), String> {
    let mut iv_bytes = [0u8; IV_SIZE];
    if iv.len() != IV_SIZE {
        return Err(format!("iv must be {IV_SIZE} bytes, got {}", iv.len()));
    }
    iv_bytes.copy_from_slice(iv);
    let mut cipher = Aes256Ctr::new(key.into(), (&iv_bytes).into());
    cipher.seek(offset as u64);
    cipher.apply_keystream(data);
    Ok(())
}

/// The plaintext `type = -3` header event: three TL strings behind a flags word.
pub struct Header {
    pub salt: Vec<u8>,
    pub iv: Vec<u8>,
    pub hash: Vec<u8>,
}

impl Header {
    /// Fresh salt and iv, hash derived from the key -- `reset_encryption`.
    pub fn generate(key: &[u8]) -> Result<(Header, [u8; 32]), String> {
        let salt = secure_bytes(SALT_SIZE)?;
        let iv = secure_bytes(IV_SIZE)?;
        let derived = derive_key(key, &salt);
        let hash = key_hash(&derived).to_vec();
        Ok((Header { salt, iv, hash }, derived))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_FLAGS_SIZE + SALT_SIZE + IV_SIZE + KEY_HASH_SIZE + 12);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&store_tl_string(&self.salt));
        out.extend_from_slice(&store_tl_string(&self.iv));
        out.extend_from_slice(&store_tl_string(&self.hash));
        out
    }

    pub fn parse(data: &[u8]) -> Option<Header> {
        if data.len() < HEADER_FLAGS_SIZE {
            return None;
        }
        let mut rest = &data[HEADER_FLAGS_SIZE..];
        let salt = take_tl_string(&mut rest)?;
        let iv = take_tl_string(&mut rest)?;
        let hash = take_tl_string(&mut rest)?;
        Some(Header {
            salt: salt.to_vec(),
            iv: iv.to_vec(),
            hash: hash.to_vec(),
        })
    }
}