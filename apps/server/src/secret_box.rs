//! Secrets at rest (GitHub App keys, Notion import tokens): ChaCha20-Poly1305 under the
//! server's app key (`ORBIT_APP_KEY`). The stored value is `nonce (12 bytes) || ciphertext`. [`seal`] also binds
//! associated data, so a sealed value cannot be moved to another row (mail keys, mail messages, DKIM keys).

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use orbit_platform::AppKey;

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

/// Encrypts `plaintext` with a fresh random nonce, bound to `aad`.
pub(crate) fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, ()> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let encrypted = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| ())?;
    let mut result = nonce.to_vec();
    result.extend_from_slice(&encrypted);
    Ok(result)
}

/// Decrypts a value from [`seal`]; fails for another key, other associated data or altered bytes.
pub(crate) fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, ()> {
    if sealed.len() < 28 {
        return Err(());
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(
            Nonce::from_slice(&sealed[..12]),
            Payload {
                msg: &sealed[12..],
                aad,
            },
        )
        .map_err(|_| ())
}

/// Older versions created an `app.key` file beside the SQLite database when no key was configured. If it is
/// still there, `ORBIT_APP_KEY` must hold the same key, or every saved secret becomes unreadable.
pub(crate) fn check_old_app_key_file(
    key: &AppKey,
    database_path: &std::path::Path,
) -> Result<(), String> {
    let path = database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("app.key");
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
    };
    if contents.parse::<AppKey>().is_ok_and(|old| old == *key) {
        return Ok(());
    }
    Err(format!(
        "ORBIT_APP_KEY differs from the key in {path}, which encrypted the saved secrets. Set \
         ORBIT_APP_KEY to the contents of {path}, or delete the file if you will enter the secrets again",
        path = path.display()
    ))
}

#[cfg(test)]
mod tests {
    use orbit_platform::AppKey;

    use super::{check_old_app_key_file, decrypt_secret, encrypt_secret, open, seal};

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn sealed_bytes_open_with_the_same_associated_data() {
        let sealed = seal(&KEY, b"mail-raw:1", b"\x00raw bytes\xff").unwrap();
        assert_eq!(
            open(&KEY, b"mail-raw:1", &sealed).unwrap(),
            b"\x00raw bytes\xff"
        );
    }

    #[test]
    fn sealed_bytes_do_not_open_with_other_associated_data() {
        let sealed = seal(&KEY, b"mail-raw:1", b"secret").unwrap();
        assert!(open(&KEY, b"mail-raw:2", &sealed).is_err());
        assert!(open(&[8; 32], b"mail-raw:1", &sealed).is_err());
    }

    #[test]
    fn truncated_sealed_bytes_do_not_open() {
        let sealed = seal(&KEY, b"a", b"secret").unwrap();
        assert!(open(&KEY, b"a", &sealed[..sealed.len() - 1]).is_err());
        assert!(open(&KEY, b"a", &sealed[..11]).is_err());
    }

    #[test]
    fn secrets_sealed_without_associated_data_still_decrypt() {
        let encrypted = encrypt_secret(&KEY, "token").unwrap();
        assert_eq!(decrypt_secret(&KEY, &encrypted).unwrap(), "token");
        assert!(open(&KEY, b"", &encrypted).is_ok());
    }

    #[test]
    fn a_secret_opens_only_with_its_key() {
        let sealed = encrypt_secret(&[1; 32], "smtp password").unwrap();
        assert_eq!(decrypt_secret(&[1; 32], &sealed).unwrap(), "smtp password");
        assert!(decrypt_secret(&[2; 32], &sealed).is_err());
    }

    #[test]
    fn an_old_app_key_file_must_match_the_configured_key() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("orbit.sqlite");
        let key = AppKey::new([1; 32]);
        check_old_app_key_file(&key, &database).unwrap();

        std::fs::write(
            directory.path().join("app.key"),
            "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\n",
        )
        .unwrap();
        check_old_app_key_file(&key, &database).unwrap();

        let error = check_old_app_key_file(&AppKey::new([2; 32]), &database).unwrap_err();
        assert!(error.contains("app.key"), "{error}");
    }
}
