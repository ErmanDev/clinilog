//! Encryption for the offline cache.
//!
//! Every sensitive value is sealed with AES-256-GCM before it touches disk. The
//! "associated data" binds each blob to its table and row id, so encrypted rows
//! can't be swapped between rows or tables without failing to open.
//!
//! The 256-bit key is random, created once per PC, and stored protected by
//! Windows DPAPI (current user), so a copied cache file can't be read on
//! another PC or by another Windows account.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
    Aes256Gcm, Key, Nonce,
};
use std::path::Path;

const NONCE_LEN: usize = 12;
const KEY_FILE: &str = "offline-cache.key";

pub struct Cipher {
    aead: Aes256Gcm,
}

#[derive(Debug)]
pub struct CryptoError(pub String);

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Cipher {
    pub fn new(key: &[u8; 32]) -> Self {
        Self { aead: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)) }
    }

    /// `nonce || ciphertext+tag`. `aad` should name the table and row.
    pub fn seal(&self, aad: &str, plaintext: &[u8]) -> Vec<u8> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = self
            .aead
            .encrypt(&nonce, Payload { msg: plaintext, aad: aad.as_bytes() })
            .expect("AES-GCM encryption does not fail for in-memory buffers");
        let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        out
    }

    pub fn open(&self, aad: &str, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if blob.len() < NONCE_LEN + 16 {
            return Err(CryptoError("encrypted value is too short".into()));
        }
        let (nonce, ct) = blob.split_at(NONCE_LEN);
        self.aead
            .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: aad.as_bytes() })
            .map_err(|_| CryptoError("encrypted value could not be opened (wrong key or tampered)".into()))
    }
}

/// Loads this PC's cache key, creating it on first use.
/// Returns `(key, created)`; `created` means any existing cache can't be read and must be reset.
pub fn load_or_create_key(dir: &Path) -> Result<([u8; 32], bool), CryptoError> {
    std::fs::create_dir_all(dir).map_err(|e| CryptoError(format!("can't create app data folder: {e}")))?;
    let path = dir.join(KEY_FILE);
    if let Ok(protected) = std::fs::read(&path) {
        match unprotect(&protected) {
            Ok(raw) if raw.len() == 32 => {
                let mut key = [0u8; 32];
                key.copy_from_slice(&raw);
                return Ok((key, false));
            }
            // Unreadable key (e.g. copied from another PC/user): start a fresh cache.
            _ => {}
        }
    }
    let key: [u8; 32] = Aes256Gcm::generate_key(&mut OsRng).into();
    let protected = protect(&key)?;
    std::fs::write(&path, protected).map_err(|e| CryptoError(format!("can't save cache key: {e}")))?;
    Ok((key, true))
}

#[cfg(windows)]
fn protect(data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    dpapi::protect(data)
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    dpapi::unprotect(data)
}

// Non-Windows builds (development only): the key is stored as-is.
#[cfg(not(windows))]
fn protect(data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    Ok(data.to_vec())
}

#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    Ok(data.to_vec())
}

#[cfg(windows)]
mod dpapi {
    use super::CryptoError;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    const ENTROPY: &[u8] = b"Clinilog offline cache v1";

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    /// Copies the output blob and frees the buffer Windows allocated for it.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        v
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| CryptoError(format!("Windows couldn't protect the cache key: {e}")))?;
            Ok(take(out))
        }
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| CryptoError(format!("Windows couldn't unlock the cache key: {e}")))?;
            Ok(take(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_round_trip_and_tamper_detection() {
        let c = Cipher::new(&[7u8; 32]);
        let blob = c.seal("students:abc", b"Dela Cruz, Juan");
        assert_eq!(c.open("students:abc", &blob).unwrap(), b"Dela Cruz, Juan");
        // Plaintext is not visible in the stored bytes.
        assert!(!blob.windows(4).any(|w| w == b"Juan"));
        // Moved to another row / table: refused.
        assert!(c.open("students:xyz", &blob).is_err());
        assert!(c.open("medicines:abc", &blob).is_err());
        // Flipped bit: refused.
        let mut bad = blob.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(c.open("students:abc", &bad).is_err());
        // Other key: refused.
        assert!(Cipher::new(&[8u8; 32]).open("students:abc", &blob).is_err());
        // Same plaintext twice gives different ciphertext (random nonce).
        assert_ne!(c.seal("t:1", b"x"), c.seal("t:1", b"x"));
    }

    #[test]
    fn key_is_created_once_then_reused_and_stored_protected() {
        let dir = std::env::temp_dir().join(format!("clinilog-key-test-{}", uuid::Uuid::new_v4()));
        let (k1, created1) = load_or_create_key(&dir).unwrap();
        let (k2, created2) = load_or_create_key(&dir).unwrap();
        assert!(created1 && !created2);
        assert_eq!(k1, k2);
        let on_disk = std::fs::read(dir.join(KEY_FILE)).unwrap();
        #[cfg(windows)]
        assert!(!on_disk.windows(32).any(|w| w == k1), "raw key must not be on disk");
        let _ = on_disk;
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unreadable_key_file_starts_a_fresh_cache() {
        let dir = std::env::temp_dir().join(format!("clinilog-key-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(KEY_FILE), b"garbage from another PC").unwrap();
        let (_, created) = load_or_create_key(&dir).unwrap();
        #[cfg(windows)]
        assert!(created);
        let _ = created;
        std::fs::remove_dir_all(dir).ok();
    }
}
