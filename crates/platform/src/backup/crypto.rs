//! Backup encryption. Each backup has its own random data key, sealed with the app key (associated data
//! `backup:` + the backup id) and stored in the plaintext manifest. Every file of the snapshot is the
//! STREAM construction (`aead::stream`, big-endian 32-bit counter) of ChaCha20-Poly1305: a random 7-byte
//! nonce prefix, then 64 KiB chunks with a 16-byte tag each; the last chunk (0 to 64 KiB) is marked as
//! last, so a truncated file does not decrypt. Each chunk's associated data is the file's path in the
//! snapshot, so files cannot be swapped. <https://docs.rs/aead/0.5.2/aead/stream/index.html>

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::stream::{DecryptorBE32, EncryptorBE32};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use rand::rngs::OsRng;
use tokio_util::sync::CancellationToken;

use super::{BackupError, check_cancelled, io_error};

pub(super) const ALGORITHM: &str = "chacha20poly1305-stream";
const CHUNK: usize = 64 * 1024;
const TAG: usize = 16;
const NONCE_PREFIX: usize = 7;
const SEAL_NONCE: usize = 12;
const READ_BUFFER: usize = 1024 * 1024;

/// The random key of one backup.
#[derive(Clone)]
pub(super) struct DataKey(Key);

impl fmt::Debug for DataKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DataKey(<redacted>)")
    }
}

fn key_aad(backup_id: &str) -> Vec<u8> {
    format!("backup:{backup_id}").into_bytes()
}

impl DataKey {
    pub(super) fn generate() -> Self {
        let mut key = Key::default();
        OsRng.fill_bytes(&mut key);
        Self(key)
    }

    /// Base64 of `nonce(12) || ciphertext`.
    pub(super) fn seal(&self, app_key: &[u8; 32], backup_id: &str) -> Result<String, BackupError> {
        let mut nonce = Nonce::default();
        OsRng.fill_bytes(&mut nonce);
        let aad = key_aad(backup_id);
        let sealed = ChaCha20Poly1305::new(Key::from_slice(app_key))
            .encrypt(
                &nonce,
                Payload {
                    msg: self.0.as_slice(),
                    aad: &aad,
                },
            )
            .map_err(|_| BackupError::Crypto {
                path: "manifest.json".to_owned(),
            })?;
        let mut output = nonce.to_vec();
        output.extend_from_slice(&sealed);
        Ok(STANDARD.encode(output))
    }

    /// `None` when `sealed` was not sealed with `app_key` for `backup_id`.
    pub(super) fn open(sealed: &str, app_key: &[u8; 32], backup_id: &str) -> Option<Self> {
        let sealed = STANDARD.decode(sealed).ok()?;
        if sealed.len() < SEAL_NONCE {
            return None;
        }
        let (nonce, ciphertext) = sealed.split_at(SEAL_NONCE);
        let aad = key_aad(backup_id);
        let key = ChaCha20Poly1305::new(Key::from_slice(app_key))
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .ok()?;
        (key.len() == 32).then(|| Self(*Key::from_slice(&key)))
    }

    /// Encrypts the file stored at `path` (relative to the snapshot, `/`-separated).
    pub(super) fn sealer(&self, path: &str) -> Sealer {
        let mut prefix = [0_u8; NONCE_PREFIX];
        OsRng.fill_bytes(&mut prefix);
        Sealer {
            stream: EncryptorBE32::from_aead(
                ChaCha20Poly1305::new(&self.0),
                prefix.as_slice().into(),
            ),
            header: Some(prefix),
            buffer: Vec::with_capacity(CHUNK),
            path: path.to_owned(),
        }
    }

    /// Decrypts the file stored at `path` (relative to the snapshot, `/`-separated).
    pub(super) fn opener(&self, path: &str) -> Opener {
        Opener {
            key: self.0,
            prefix: Vec::with_capacity(NONCE_PREFIX),
            stream: None,
            buffer: Vec::with_capacity(CHUNK + TAG),
            path: path.to_owned(),
        }
    }
}

/// Encrypts a file pushed in pieces of any size. A full chunk is sealed only once more data arrives, so
/// [`Sealer::finish`] can mark the last one.
pub(super) struct Sealer {
    stream: EncryptorBE32<ChaCha20Poly1305>,
    header: Option<[u8; NONCE_PREFIX]>,
    buffer: Vec<u8>,
    path: String,
}

impl Sealer {
    /// Appends the ciphertext that `data` completes to `output`.
    pub(super) fn update(
        &mut self,
        mut data: &[u8],
        output: &mut Vec<u8>,
    ) -> Result<(), BackupError> {
        if let Some(header) = self.header.take() {
            output.extend_from_slice(&header);
        }
        while !data.is_empty() {
            if self.buffer.len() == CHUNK {
                let sealed = self
                    .stream
                    .encrypt_next(Payload {
                        msg: &self.buffer,
                        aad: self.path.as_bytes(),
                    })
                    .map_err(|_| crypto_error(&self.path))?;
                output.extend_from_slice(&sealed);
                self.buffer.clear();
            }
            let take = (CHUNK - self.buffer.len()).min(data.len());
            self.buffer.extend_from_slice(&data[..take]);
            data = &data[take..];
        }
        Ok(())
    }

    pub(super) fn finish(mut self, output: &mut Vec<u8>) -> Result<(), BackupError> {
        if let Some(header) = self.header.take() {
            output.extend_from_slice(&header);
        }
        let sealed = self
            .stream
            .encrypt_last(Payload {
                msg: &self.buffer,
                aad: self.path.as_bytes(),
            })
            .map_err(|_| crypto_error(&self.path))?;
        output.extend_from_slice(&sealed);
        Ok(())
    }
}

/// Decrypts a file pushed in pieces of any size; the mirror of [`Sealer`].
pub(super) struct Opener {
    key: Key,
    prefix: Vec<u8>,
    stream: Option<DecryptorBE32<ChaCha20Poly1305>>,
    buffer: Vec<u8>,
    path: String,
}

impl Opener {
    /// Appends the plaintext that `data` completes to `output`.
    pub(super) fn update(
        &mut self,
        mut data: &[u8],
        output: &mut Vec<u8>,
    ) -> Result<(), BackupError> {
        if self.stream.is_none() {
            let take = (NONCE_PREFIX - self.prefix.len()).min(data.len());
            self.prefix.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.prefix.len() < NONCE_PREFIX {
                return Ok(());
            }
            self.stream = Some(DecryptorBE32::from_aead(
                ChaCha20Poly1305::new(&self.key),
                self.prefix.as_slice().into(),
            ));
        }
        let stream = self.stream.as_mut().expect("the stream starts above");
        while !data.is_empty() {
            if self.buffer.len() == CHUNK + TAG {
                let opened = stream
                    .decrypt_next(Payload {
                        msg: &self.buffer,
                        aad: self.path.as_bytes(),
                    })
                    .map_err(|_| crypto_error(&self.path))?;
                output.extend_from_slice(&opened);
                self.buffer.clear();
            }
            let take = (CHUNK + TAG - self.buffer.len()).min(data.len());
            self.buffer.extend_from_slice(&data[..take]);
            data = &data[take..];
        }
        Ok(())
    }

    pub(super) fn finish(self, output: &mut Vec<u8>) -> Result<(), BackupError> {
        let stream = self.stream.ok_or_else(|| crypto_error(&self.path))?;
        let opened = stream
            .decrypt_last(Payload {
                msg: &self.buffer,
                aad: self.path.as_bytes(),
            })
            .map_err(|_| crypto_error(&self.path))?;
        output.extend_from_slice(&opened);
        Ok(())
    }
}

fn crypto_error(path: &str) -> BackupError {
    BackupError::Crypto {
        path: path.to_owned(),
    }
}

/// Writes `source` encrypted as the snapshot file `path` to `destination`.
pub(super) fn encrypt_file(
    source: &Path,
    destination: &Path,
    key: &DataKey,
    path: &str,
    cancellation: &CancellationToken,
) -> Result<(), BackupError> {
    let mut input = File::open(source).map_err(|error| io_error(source, error))?;
    let mut output = File::create(destination).map_err(|error| io_error(destination, error))?;
    let mut sealer = key.sealer(path);
    let mut buffer = vec![0_u8; READ_BUFFER];
    let mut sealed = Vec::with_capacity(READ_BUFFER + READ_BUFFER / CHUNK * TAG + CHUNK);
    loop {
        check_cancelled(cancellation)?;
        let read = input
            .read(&mut buffer)
            .map_err(|error| io_error(source, error))?;
        if read == 0 {
            break;
        }
        sealer.update(&buffer[..read], &mut sealed)?;
        output
            .write_all(&sealed)
            .map_err(|error| io_error(destination, error))?;
        sealed.clear();
    }
    sealer.finish(&mut sealed)?;
    output
        .write_all(&sealed)
        .and_then(|()| output.sync_all())
        .map_err(|error| io_error(destination, error))
}

/// Writes the snapshot file `source` (stored as `path`) decrypted to the new file `destination`.
pub(super) fn decrypt_file(
    source: &Path,
    destination: &Path,
    key: &DataKey,
    path: &str,
) -> Result<(), BackupError> {
    let mut input = File::open(source).map_err(|error| io_error(source, error))?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| io_error(destination, error))?;
    let mut opener = key.opener(path);
    let mut buffer = vec![0_u8; READ_BUFFER];
    let mut opened = Vec::with_capacity(READ_BUFFER + CHUNK);
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| io_error(source, error))?;
        if read == 0 {
            break;
        }
        opener.update(&buffer[..read], &mut opened)?;
        output
            .write_all(&opened)
            .map_err(|error| io_error(destination, error))?;
        opened.clear();
    }
    opener.finish(&mut opened)?;
    output
        .write_all(&opened)
        .and_then(|()| output.sync_all())
        .map_err(|error| io_error(destination, error))
}

#[cfg(test)]
mod tests {
    use super::{BackupError, CHUNK, DataKey, NONCE_PREFIX, TAG};

    fn seal(key: &DataKey, path: &str, data: &[u8], piece: usize) -> Vec<u8> {
        let mut sealer = key.sealer(path);
        let mut output = Vec::new();
        for part in data.chunks(piece.max(1)) {
            sealer.update(part, &mut output).unwrap();
        }
        sealer.finish(&mut output).unwrap();
        output
    }

    fn open(key: &DataKey, path: &str, data: &[u8], piece: usize) -> Result<Vec<u8>, BackupError> {
        let mut opener = key.opener(path);
        let mut output = Vec::new();
        for part in data.chunks(piece.max(1)) {
            opener.update(part, &mut output)?;
        }
        opener.finish(&mut output)?;
        Ok(output)
    }

    #[test]
    fn round_trips_any_size_in_any_pieces() {
        let key = DataKey::generate();
        for size in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK, 3 * CHUNK + 5] {
            let data: Vec<u8> = (0..size).map(|index| (index % 253) as u8).collect();
            for piece in [1_000, 7, CHUNK, CHUNK + TAG, 1 << 20] {
                let sealed = seal(&key, "attachments/a", &data, piece);
                assert_eq!(
                    sealed.len(),
                    NONCE_PREFIX + data.len() + (data.len().max(1)).div_ceil(CHUNK) * TAG,
                    "size {size}"
                );
                assert_eq!(open(&key, "attachments/a", &sealed, piece).unwrap(), data);
            }
        }
    }

    #[test]
    fn truncation_tampering_and_another_path_fail() {
        let key = DataKey::generate();
        let data = vec![3_u8; 2 * CHUNK + 10];
        let sealed = seal(&key, "database.sqlite", &data, 4096);
        // Dropping the last chunk leaves a stream without its last chunk.
        let truncated = &sealed[..NONCE_PREFIX + 2 * (CHUNK + TAG)];
        assert!(open(&key, "database.sqlite", truncated, 4096).is_err());
        let mut flipped = sealed.clone();
        flipped[NONCE_PREFIX + 5] ^= 1;
        assert!(open(&key, "database.sqlite", &flipped, 4096).is_err());
        assert!(open(&key, "attachments/database.sqlite", &sealed, 4096).is_err());
        assert!(open(&DataKey::generate(), "database.sqlite", &sealed, 4096).is_err());
        assert!(open(&key, "database.sqlite", &sealed[..3], 4096).is_err());
    }

    #[test]
    fn the_sealed_key_opens_only_with_its_app_key_and_backup_id() {
        let key = DataKey::generate();
        let sealed = key.seal(&[1; 32], "backup-a").unwrap();
        let opened = DataKey::open(&sealed, &[1; 32], "backup-a").unwrap();
        assert_eq!(opened.0, key.0);
        assert!(DataKey::open(&sealed, &[2; 32], "backup-a").is_none());
        assert!(DataKey::open(&sealed, &[1; 32], "backup-b").is_none());
        assert!(DataKey::open("not base64!", &[1; 32], "backup-a").is_none());
    }
}
