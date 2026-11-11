use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use mail_auth::common::crypto::{RsaKey, Sha256};
use mail_auth::common::headers::HeaderWriter as _;
use mail_auth::dkim::DkimSigner;
use orbit_mail::{Authenticator, StaticDns, Verdict, dkim_txt, generate_dkim_key, new_selector};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

#[test]
fn selectors_count_up_in_letters() {
    assert_eq!(new_selector(2026, 0), "o2026a");
    assert_eq!(new_selector(2026, 1), "o2026b");
    assert_eq!(new_selector(2026, 25), "o2026z");
    assert_eq!(new_selector(2026, 26), "o2026aa");
    assert_eq!(new_selector(2027, 27), "o2027ab");
    assert_eq!(new_selector(2026, 701), "o2026zz");
    assert_eq!(new_selector(2026, 702), "o2026aaa");
}

#[test]
fn dkim_txt_has_the_record_shape() {
    assert_eq!(dkim_txt(&[1, 2, 3]), "v=DKIM1; k=rsa; p=AQID");
}

/// RSA-2048 generation is slow in debug builds, so one test covers the whole key: it is RSA-2048, its private half
/// signs through mail-auth (what slice 2 uses) and the TXT record from `dkim_txt` verifies that signature.
#[tokio::test]
async fn generated_key_signs_and_its_txt_record_verifies() {
    let key = generate_dkim_key();
    assert!(format!("{key:?}").contains("<redacted>"));

    let public =
        <rsa::RsaPublicKey as rsa::pkcs8::DecodePublicKey>::from_public_key_der(&key.public_der)
            .unwrap();
    assert_eq!(rsa::traits::PublicKeyParts::size(&public), 256);
    let private = <rsa::RsaPrivateKey as rsa::pkcs8::DecodePrivateKey>::from_pkcs8_der(
        &key.private_pkcs8_der,
    )
    .unwrap();
    assert_eq!(private.to_public_key(), public);

    let raw = "From: Ann <ann@example.com>\r\nTo: support@orbit.test\r\nSubject: Hello\r\n\
               Date: Thu, 22 Oct 2026 10:00:00 +0000\r\nMessage-ID: <1@example.com>\r\n\r\nhi\r\n";
    let signer = RsaKey::<Sha256>::from_key_der(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        key.private_pkcs8_der.as_slice(),
    )))
    .unwrap();
    let selector = new_selector(2026, 0);
    let signature = DkimSigner::from_key(signer)
        .domain("example.com")
        .selector(&selector)
        .headers(["From", "To", "Subject", "Date", "Message-ID"])
        .sign(raw.as_bytes())
        .unwrap();
    let signed = format!("{}{raw}", signature.to_header());

    let record = dkim_txt(&key.public_der);
    assert!(record.starts_with("v=DKIM1; k=rsa; p=MIIBIjAN"), "{record}");
    let dns = StaticDns::new().txt(&format!("{selector}._domainkey.example.com"), &record);
    let authenticator = Authenticator::with_static_dns(dns).unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        authenticator.check(
            signed.as_bytes(),
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            "mail.example.com",
            Some("ann@example.com"),
        ),
    )
    .await
    .unwrap();
    assert_eq!(outcome.dkim, Verdict::Pass);
    assert_eq!(outcome.dkim_domain.as_deref(), Some("example.com"));
}
