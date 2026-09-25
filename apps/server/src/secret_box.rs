//! Secrets at rest (GitHub App keys, Notion import tokens): ChaCha20-Poly1305 under the
//! server's app key (`secrets.app_key`). The stored value is `nonce (12 bytes) || ciphertext`.

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

/// Encrypts `value` with a fresh random nonce.
pub(crate) fn encrypt_secret(key: &[u8; 32], value: &str) -> Result<Vec<u8>, ()> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let encrypted = cipher.encrypt(&nonce, value.as_bytes()).map_err(|_| ())?;
    let mut result = nonce.to_vec();
    result.extend_from_slice(&encrypted);
    Ok(result)
}

/// Decrypts a value from [`encrypt_secret`]; fails for another key or altered bytes.
pub(crate) fn decrypt_secret(key: &[u8; 32], value: &[u8]) -> Result<String, ()> {
    if value.len() < 28 {
        return Err(());
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&value[..12]), &value[12..])
        .map_err(|_| ())?;
    String::from_utf8(plaintext).map_err(|_| ())
}
