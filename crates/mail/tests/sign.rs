use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use orbit_mail::{
    Authenticator, Composed, ComposedAddress, SIGNED_HEADERS, SignError, SigningKey, StaticDns,
    Verdict, build_message, dkim_txt, generate_dkim_key, sign_message,
};

fn addr(name: Option<&str>, address: &str) -> ComposedAddress {
    ComposedAddress {
        name: name.map(str::to_owned),
        address: address.to_owned(),
    }
}

fn composed(subject: &str, text: &str) -> Composed {
    Composed {
        message_id: "<m1@example.com>".into(),
        date: 1_794_000_000,
        from: addr(Some("Ann"), "ann@example.com"),
        to: vec![addr(None, "bob@example.net")],
        cc: vec![],
        reply_to: None,
        subject: subject.into(),
        in_reply_to: None,
        references: vec![],
        html: format!("<p>{text}</p>"),
        text: text.into(),
        files: vec![],
        extra_headers: vec![],
    }
}

/// RSA-2048 generation is slow in debug builds, so every test shares one key.
fn key() -> &'static (SigningKey, String) {
    static KEY: std::sync::OnceLock<(SigningKey, String)> = std::sync::OnceLock::new();
    KEY.get_or_init(|| {
        let generated = generate_dkim_key();
        (
            SigningKey {
                domain: "example.com".into(),
                selector: "s".into(),
                pkcs8_der: generated.private_pkcs8_der.clone(),
            },
            dkim_txt(&generated.public_der),
        )
    })
}

async fn verify(raw: &[u8]) -> Verdict {
    let dns = StaticDns::new().txt("s._domainkey.example.com", &key().1);
    let authenticator = Authenticator::with_static_dns(dns).unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        authenticator.check(
            raw,
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            "mail.example.com",
            Some("ann@example.com"),
        ),
    )
    .await
    .unwrap()
    .dkim
}

fn signed(c: &Composed) -> Vec<u8> {
    sign_message(&build_message(c).unwrap(), &key().0).unwrap()
}

fn replace_line(raw: &[u8], prefix: &str, new: &str) -> Vec<u8> {
    let text = String::from_utf8(raw.to_vec()).unwrap();
    text.split("\r\n")
        .map(|line| {
            if line.starts_with(prefix) {
                new.to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n")
        .into_bytes()
}

#[tokio::test]
async fn signature_verifies() {
    let raw = signed(&composed("Hello", "Hi there"));
    assert!(raw.starts_with(b"DKIM-Signature: "));
    assert_eq!(verify(&raw).await, Verdict::Pass);
}

#[tokio::test]
async fn still_verifies_after_message_id_and_date_change() {
    let raw = signed(&composed("Hello", "Hi there"));
    let raw = replace_line(&raw, "Message-ID:", "Message-ID: <other@provider.test>");
    let raw = replace_line(&raw, "Date:", "Date: Fri, 23 Oct 2026 10:00:00 +0000");
    assert_eq!(verify(&raw).await, Verdict::Pass);
}

#[tokio::test]
async fn still_verifies_after_provider_header_added() {
    let raw = signed(&composed("Hello", "Hi there"));
    let mut with = b"X-PM-KeepID: true\r\n".to_vec();
    with.extend_from_slice(&raw);
    assert_eq!(verify(&with).await, Verdict::Pass);
}

#[tokio::test]
async fn fails_after_body_change() {
    let raw = signed(&composed("Hello", "Hi there"));
    let text = String::from_utf8(raw).unwrap();
    assert_eq!(
        verify(text.replace("Hi there", "Hi thera").as_bytes()).await,
        Verdict::Fail
    );
}

#[test]
fn h_tag_lists_from_twice_and_not_message_id() {
    let raw = signed(&composed("Hello", "Hi there"));
    let text = String::from_utf8(raw).unwrap();
    let header = text
        .split("\r\n")
        .scan(false, |done, line| {
            if *done
                || (!line.starts_with(' ') && !line.starts_with('\t') && !line.starts_with("DKIM-"))
            {
                *done = true;
                return None;
            }
            Some(line.trim())
        })
        .collect::<String>()
        .to_ascii_lowercase();
    let header = header.replace(' ', "");
    let h = header
        .split(';')
        .map(str::trim)
        .find_map(|tag| tag.strip_prefix("h="))
        .unwrap()
        .split(':')
        .map(|name| name.trim().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(h.iter().filter(|name| *name == "from").count(), 2, "{h:?}");
    assert!(!h.iter().any(|name| name == "message-id" || name == "date"));
    assert!(header.contains("c=relaxed/relaxed"), "{header}");
    assert!(header.contains("a=rsa-sha256"), "{header}");
    assert_eq!(SIGNED_HEADERS.iter().filter(|n| **n == "From").count(), 2);
}

#[tokio::test]
async fn non_ascii_message_verifies() {
    let raw = signed(&composed("Grüße aus Zürich ✓", "Héllo wörld, 你好 🙂"));
    assert_eq!(verify(&raw).await, Verdict::Pass);
}

#[test]
fn bad_key_is_an_error_and_debug_redacts() {
    let bad = SigningKey {
        domain: "example.com".into(),
        selector: "s".into(),
        pkcs8_der: vec![1, 2, 3],
    };
    assert!(matches!(
        sign_message(b"From: a@b.c\r\n\r\nx\r\n", &bad),
        Err(SignError::Key)
    ));
    assert!(format!("{:?}", key().0).contains("<redacted>"));
}
