use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::rand_core::RngCore,
    aead::{Aead, KeyInit, OsRng},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::fmt;

use crate::config;

/// Errors that can occur during encryption/decryption operations.
#[derive(Debug)]
pub enum CryptoError {
    /// The encryption key in the config is missing or empty.
    MissingKey,
    /// The encryption key is not valid hex or not the correct length (32 bytes / 64 hex chars).
    InvalidKey(String),
    /// AES-GCM encryption failed.
    EncryptionFailed,
    /// AES-GCM decryption failed (wrong key, corrupted data, or tampered ciphertext).
    DecryptionFailed,
    /// The ciphertext format is invalid (e.g., too short to contain a nonce).
    InvalidCiphertext,
    /// Base64 decoding failed.
    Base64DecodeFailed(String),
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoError::MissingKey => write!(
                f,
                "Encryption key is not configured. Set [security] encryption_key in config file."
            ),
            CryptoError::InvalidKey(msg) => write!(f, "Invalid encryption key: {}", msg),
            CryptoError::EncryptionFailed => write!(f, "AES-256-GCM encryption failed"),
            CryptoError::DecryptionFailed => write!(
                f,
                "AES-256-GCM decryption failed (wrong key or corrupted data)"
            ),
            CryptoError::InvalidCiphertext => write!(f, "Invalid ciphertext format"),
            CryptoError::Base64DecodeFailed(msg) => write!(f, "Base64 decode failed: {}", msg),
        }
    }
}

impl std::error::Error for CryptoError {}

/// The nonce size for AES-256-GCM (96 bits = 12 bytes).
const NONCE_SIZE: usize = 12;

/// Parse the hex-encoded encryption key from config into a 32-byte key.
fn get_encryption_key() -> Result<Key<Aes256Gcm>, CryptoError> {
    let cfg = config::get_config();
    let hex_key = &cfg.security.encryption_key;

    if hex_key.is_empty() {
        return Err(CryptoError::MissingKey);
    }

    let key_bytes = hex::decode(hex_key)
        .map_err(|e| CryptoError::InvalidKey(format!("not valid hex: {}", e)))?;

    if key_bytes.len() != 32 {
        return Err(CryptoError::InvalidKey(format!(
            "expected 32 bytes (64 hex chars), got {} bytes ({} hex chars)",
            key_bytes.len(),
            hex_key.len()
        )));
    }

    Ok(*Key::<Aes256Gcm>::from_slice(&key_bytes))
}

/// Encrypt a plaintext string using AES-256-GCM.
///
/// Returns a base64-encoded string containing `nonce || ciphertext`.
/// The encryption key is read from the global config `[security] encryption_key`.
pub fn encrypt(plaintext: &str) -> Result<String, CryptoError> {
    let key = get_encryption_key()?;
    let cipher = Aes256Gcm::new(&key);

    // Generate a random 96-bit nonce
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| CryptoError::EncryptionFailed)?;

    // Prepend nonce to ciphertext and base64-encode
    let mut combined = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// Decrypt a base64-encoded ciphertext string that was encrypted with [`encrypt`].
///
/// Expects the input to be base64-encoded `nonce || ciphertext`.
/// The encryption key is read from the global config `[security] encryption_key`.
pub fn decrypt(encrypted: &str) -> Result<String, CryptoError> {
    let key = get_encryption_key()?;
    let cipher = Aes256Gcm::new(&key);

    // Base64-decode
    let combined = BASE64
        .decode(encrypted)
        .map_err(|e| CryptoError::Base64DecodeFailed(e.to_string()))?;

    // Must have at least nonce + 1 byte of ciphertext
    if combined.len() <= NONCE_SIZE {
        return Err(CryptoError::InvalidCiphertext);
    }

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_SIZE);
    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::DecryptionFailed)?;

    String::from_utf8(plaintext).map_err(|_| CryptoError::DecryptionFailed)
}

/// Check whether encryption is configured (i.e., the encryption key is set).
pub fn is_encryption_configured() -> bool {
    let cfg = config::get_config();
    !cfg.security.encryption_key.is_empty()
}

/// Encrypt a value if encryption is configured, otherwise return it as-is.
pub fn encrypt_if_configured(plaintext: &str) -> Result<String, CryptoError> {
    if is_encryption_configured() {
        encrypt(plaintext)
    } else {
        Ok(plaintext.to_string())
    }
}

/// Decrypt a value if encryption is configured, otherwise return it as-is.
pub fn decrypt_if_configured(value: &str) -> Result<String, CryptoError> {
    if is_encryption_configured() {
        decrypt(value)
    } else {
        Ok(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require the config to be loaded with a valid encryption key.
    // In a real test environment, you would set up the config before running these tests.

    #[test]
    fn test_nonce_size() {
        assert_eq!(NONCE_SIZE, 12);
    }
}
