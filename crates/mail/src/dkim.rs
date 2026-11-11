//! DKIM keys: RSA-2048 key generation and the DNS record text (spec §4.1, D5). Slice 1 only makes the first key so
//! the setup page can show its record; signing comes in slice 2 (through `mail-auth`, which loads the PKCS#8 key).
//!
//! The `rsa` crate is used for key generation only. RUSTSEC-2023-0071 (Marvin timing attack) affects decryption and
//! signing with `rsa`, not key generation.

use base64::Engine as _;
use rsa::RsaPrivateKey;
use rsa::pkcs8::{EncodePrivateKey as _, EncodePublicKey as _};
use rsa::rand_core::OsRng;

const KEY_BITS: usize = 2048;

/// A new DKIM key pair. The private half is PKCS#8 DER (seal it before it is stored); the public half is the
/// SubjectPublicKeyInfo DER that goes into the `p=` tag.
#[derive(Clone)]
pub struct DkimKey {
    pub private_pkcs8_der: Vec<u8>,
    pub public_der: Vec<u8>,
}

impl std::fmt::Debug for DkimKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DkimKey")
            .field("private_pkcs8_der", &"<redacted>")
            .field("public_der", &self.public_der.len())
            .finish()
    }
}

/// Generates an RSA-2048 key from the OS random source. Panics only if the OS random source fails.
pub fn generate_dkim_key() -> DkimKey {
    let private = RsaPrivateKey::new(&mut OsRng, KEY_BITS)
        .expect("RSA key generation needs the OS random source");
    let private_pkcs8_der = private
        .to_pkcs8_der()
        .expect("a generated RSA key encodes as PKCS#8")
        .as_bytes()
        .to_vec();
    let public_der = private
        .to_public_key()
        .to_public_key_der()
        .expect("an RSA public key encodes as SPKI")
        .into_vec();
    DkimKey {
        private_pkcs8_der,
        public_der,
    }
}

/// The TXT record value for `{selector}._domainkey.{domain}` (RFC 6376 §3.6.1). It is longer than 255 bytes, so the
/// DNS record splits it into several strings.
pub fn dkim_txt(public_der: &[u8]) -> String {
    format!(
        "v=DKIM1; k=rsa; p={}",
        base64::engine::general_purpose::STANDARD.encode(public_der)
    )
}

/// The selector for the `n`-th key made in `year` (from 0): `o2026a`, `o2026b`, …, `o2026z`, `o2026aa`.
pub fn new_selector(year: i32, n: u32) -> String {
    let mut letters = Vec::new();
    let mut rest = u64::from(n) + 1;
    while rest > 0 {
        rest -= 1;
        letters.push(b'a' + (rest % 26) as u8);
        rest /= 26;
    }
    letters.reverse();
    format!(
        "o{year}{}",
        String::from_utf8(letters).expect("ASCII letters")
    )
}
