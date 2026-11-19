//! DKIM signing of outgoing messages (spec §4.1, slice 2). The signature is `rsa-sha256` with `relaxed/relaxed`
//! canonicalization, made with `mail-auth` 0.13: <https://docs.rs/mail-auth/0.13.3/mail_auth/dkim/index.html>
//! (`DkimSigner::from_key(..).domain(..).selector(..).headers(..).sign(raw)`, with the ring-backed
//! `common::crypto::RsaKey::<Sha256>::from_key_der`, which loads the PKCS#8 DER from `generate_dkim_key`).
//!
//! `Message-ID`, `Date`, `Return-Path` and `Bounces-To` are not signed, because a provider may rewrite them.
//! `From` is listed twice (oversigning), so nobody can add a second `From` header after signing.

use mail_auth::common::crypto::{RsaKey, Sha256};
use mail_auth::common::headers::HeaderWriter as _;
use mail_auth::dkim::DkimSigner;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

/// The headers the signature covers, in `h=` order. `From` twice is deliberate.
pub const SIGNED_HEADERS: &[&str] = &[
    "From",
    "From",
    "To",
    "Cc",
    "Subject",
    "Reply-To",
    "In-Reply-To",
    "References",
    "MIME-Version",
    "Content-Type",
];

/// A domain's DKIM key: the `d=` domain, the `s=` selector and the unsealed PKCS#8 DER private key.
#[derive(Clone)]
pub struct SigningKey {
    pub domain: String,
    pub selector: String,
    pub pkcs8_der: Vec<u8>,
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SigningKey")
            .field("domain", &self.domain)
            .field("selector", &self.selector)
            .field("pkcs8_der", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignError {
    #[error("the DKIM private key could not be loaded")]
    Key,
    #[error("the message could not be signed")]
    Sign,
}

/// Returns `raw` with a `DKIM-Signature` header in front of it.
pub fn sign_message(raw: &[u8], key: &SigningKey) -> Result<Vec<u8>, SignError> {
    use mail_auth::dkim::Canonicalization::Relaxed;
    let rsa = RsaKey::<Sha256>::from_key_der(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        key.pkcs8_der.as_slice(),
    )))
    .map_err(|_| SignError::Key)?;
    let signature = DkimSigner::from_key(rsa)
        .domain(key.domain.as_str())
        .selector(key.selector.as_str())
        .headers(SIGNED_HEADERS.iter().copied())
        .header_canonicalization(Relaxed)
        .body_canonicalization(Relaxed)
        .sign(raw)
        .map_err(|_| SignError::Sign)?;
    let header = signature.to_header();
    let mut out = Vec::with_capacity(header.len() + raw.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(raw);
    Ok(out)
}
