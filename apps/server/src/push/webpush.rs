//! The Web Push protocol: message encryption (RFC 8291, `aes128gcm` of RFC 8188) and the VAPID
//! `Authorization` header (RFC 8292). Pure functions; [`super::WebPushSender`] does the HTTP.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::{PublicKey, SecretKey};
use serde_json::json;
use sha2::Sha256;

/// The largest notice that is sent. A push service must accept 4096 bytes of ciphertext, and
/// rejects more.
pub const MAX_PAYLOAD_BYTES: usize = 3000;
const RECORD_SIZE: u32 = 4096;

/// The body of a push message for a browser's subscription keys: `ua_public` is its `p256dh`
/// key (an uncompressed P-256 point) and `auth` its 16-byte secret. `sender` is a key made for
/// this message only and `salt` 16 random bytes. `None` when a key is not valid.
#[must_use]
pub fn encrypt(
    ua_public: &[u8],
    auth: &[u8],
    payload: &[u8],
    sender: &SecretKey,
    salt: &[u8; 16],
) -> Option<Vec<u8>> {
    let receiver = PublicKey::from_sec1_bytes(ua_public).ok()?;
    let sender_public = sender.public_key().to_sec1_bytes();
    let shared = p256::ecdh::diffie_hellman(sender.to_nonzero_scalar(), receiver.as_affine());

    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(ua_public);
    info.extend_from_slice(&sender_public);
    let mut input = [0_u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes())
        .expand(&info, &mut input)
        .ok()?;

    let keys = Hkdf::<Sha256>::new(Some(salt), &input);
    let (mut key, mut nonce) = ([0_u8; 16], [0_u8; 12]);
    keys.expand(b"Content-Encoding: aes128gcm\0", &mut key)
        .ok()?;
    keys.expand(b"Content-Encoding: nonce\0", &mut nonce).ok()?;

    // One record: the payload and the delimiter of a last record.
    let mut record = payload.to_vec();
    record.push(2);
    let sealed = Aes128Gcm::new_from_slice(&key)
        .ok()?
        .encrypt(Nonce::from_slice(&nonce), record.as_slice())
        .ok()?;

    let mut body = salt.to_vec();
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(u8::try_from(sender_public.len()).ok()?);
    body.extend_from_slice(&sender_public);
    body.extend_from_slice(&sealed);
    Some(body)
}

/// The `Authorization` header that tells a push service who sends: a token signed with the
/// server's VAPID key, for the origin of the subscription's endpoint, valid until `expires`
/// (seconds since the epoch; at most 24 hours from now). `subject` is a `mailto:` or `https:`
/// contact.
#[must_use]
pub fn vapid_authorization(key: &SecretKey, audience: &str, subject: &str, expires: i64) -> String {
    let header = BASE64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims =
        BASE64.encode(json!({ "aud": audience, "exp": expires, "sub": subject }).to_string());
    let signed = format!("{header}.{claims}");
    let signature: Signature = SigningKey::from(key).sign(signed.as_bytes());
    format!(
        "vapid t={signed}.{}, k={}",
        BASE64.encode(signature.to_bytes()),
        public_key(key)
    )
}

/// The VAPID public key as browsers take it (`applicationServerKey`).
#[must_use]
pub fn public_key(key: &SecretKey) -> String {
    BASE64.encode(key.public_key().to_sec1_bytes())
}

/// Base64url without padding, as browsers send subscription keys.
#[must_use]
pub fn decode(value: &str) -> Option<Vec<u8>> {
    BASE64.decode(value.trim_end_matches('=')).ok()
}

#[cfg(test)]
mod tests {
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;

    use super::*;

    /// The example of RFC 8291, appendix A.
    #[test]
    fn encryption_matches_the_example_of_rfc_8291() {
        let sender =
            SecretKey::from_slice(&decode("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw").unwrap())
                .unwrap();
        let ua_public = decode(
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
        )
        .unwrap();
        let auth = decode("BTBZMqHH6r4Tts7J_aSIgg").unwrap();
        let salt: [u8; 16] = decode("DGv6ra1nlYgDCS1FRnbzlw")
            .unwrap()
            .try_into()
            .unwrap();
        let body = encrypt(
            &ua_public,
            &auth,
            b"When I grow up, I want to be a watermelon",
            &sender,
            &salt,
        )
        .unwrap();
        assert_eq!(
            BASE64.encode(body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmY\
             WAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexS\
             gSxsj_Qulcy4a-fN"
        );
        assert!(encrypt(b"not a point", &auth, b"x", &sender, &salt).is_none());
    }

    #[test]
    fn the_vapid_token_is_signed_by_the_key_it_names() {
        let key = SecretKey::random(&mut rand_core::OsRng);
        let header = vapid_authorization(&key, "https://push.example", "mailto:a@b.example", 99);
        let (token, named) = header
            .strip_prefix("vapid t=")
            .unwrap()
            .split_once(", k=")
            .unwrap();
        assert_eq!(named, public_key(&key));
        let (signed, signature) = token.rsplit_once('.').unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&decode(signed.split_once('.').unwrap().1).unwrap()).unwrap();
        assert_eq!(
            claims,
            json!({ "aud": "https://push.example", "exp": 99, "sub": "mailto:a@b.example" })
        );
        let signature = Signature::from_slice(&decode(signature).unwrap()).unwrap();
        VerifyingKey::from(key.public_key())
            .verify(signed.as_bytes(), &signature)
            .unwrap();
    }
}
