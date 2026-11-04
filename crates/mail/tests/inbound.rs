//! Provider deliveries (provider intake design §2, §6): each provider's request is authenticated
//! first, then the raw message, the envelope and the SPF source are read from it.

use std::sync::OnceLock;

use base64::Engine as _;
use hmac::{Hmac, Mac};
use orbit_mail::inbound::{
    Delivery, InboundError, SpfHeaderOwned, SpfSource, cloudflare, mailgun, postmark, resend, ses,
};
use orbit_mail::{ProviderSpf, Verdict};
use rcgen::SigningKey as _;
use sha2::Sha256;

const NOW: i64 = 1_793_000_000;
const MAX: usize = 1024 * 1024;
const RAW: &str = "Received-SPF: pass (provider) envelope-from=\"ann@sender.example\";\r\n\
    Received: from mx.sender.example by mx.provider.example;\r\n\
    From: Ann <ann@sender.example>\r\nTo: team@orbit.example\r\nSubject: Hi\r\n\r\nHello\r\n";

fn hmac_hex(key: &[u8], message: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(message);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn assert_delivery(delivery: &Delivery, recipients: &[&str]) {
    assert_eq!(delivery.raw, RAW.as_bytes());
    assert_eq!(delivery.mail_from.as_deref(), Some("ann@sender.example"));
    assert_eq!(delivery.recipients, recipients);
}

// ---------------------------------------------------------------- Cloudflare

const CF_SECRET: &[u8] = b"cloudflare-signing-secret";

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The headers the Email Worker sends with the raw message as the body.
fn cloudflare_headers(raw: &[u8], to: &str, timestamp: i64, secret: &[u8]) -> [String; 4] {
    let timestamp = timestamp.to_string();
    let from = "ann%40sender.example".to_owned();
    let to = to.replace('@', "%40");
    let signature = hmac_hex(
        secret,
        format!("{timestamp}.{from}.{to}.{}", sha256_hex(raw)).as_bytes(),
    );
    [timestamp, signature, from, to]
}

fn cloudflare(headers: &[String; 4], raw: &[u8], max: usize) -> Result<Delivery, InboundError> {
    let [timestamp, signature, from, to] = headers;
    cloudflare::verify_and_parse(
        cloudflare::Envelope {
            timestamp: Some(timestamp),
            signature: Some(signature),
            from: Some(from),
            to: Some(to),
        },
        raw,
        CF_SECRET,
        NOW,
        max,
    )
}

#[test]
fn cloudflare_delivery_is_read_after_its_signature() {
    // the envelope recipient need not be in the headers (Bcc)
    let headers = cloudflare_headers(
        RAW.as_bytes(),
        "bcc-only@orbit.example",
        NOW - 10,
        CF_SECRET,
    );
    let delivery = cloudflare(&headers, RAW.as_bytes(), MAX).unwrap();
    assert_delivery(&delivery, &["bcc-only@orbit.example"]);
    assert_eq!(delivery.spf, SpfSource::Header(SpfHeaderOwned::ReceivedSpf));
}

#[test]
fn cloudflare_refuses_a_wrong_secret_a_changed_body_or_envelope_and_an_old_request() {
    let wrong = cloudflare_headers(RAW.as_bytes(), "team@orbit.example", NOW, b"another-secret");
    assert_eq!(
        cloudflare(&wrong, RAW.as_bytes(), MAX).unwrap_err(),
        InboundError::Unauthorized
    );

    let good = cloudflare_headers(RAW.as_bytes(), "team@orbit.example", NOW, CF_SECRET);
    let changed = format!("{RAW}more");
    assert_eq!(
        cloudflare(&good, changed.as_bytes(), MAX).unwrap_err(),
        InboundError::Unauthorized
    );
    let mut other_recipient = good.clone();
    other_recipient[3] = "ceo%40orbit.example".to_owned();
    assert_eq!(
        cloudflare(&other_recipient, RAW.as_bytes(), MAX).unwrap_err(),
        InboundError::Unauthorized
    );
    let missing = cloudflare::verify_and_parse(
        cloudflare::Envelope {
            timestamp: Some(&good[0]),
            signature: None,
            from: Some(&good[2]),
            to: Some(&good[3]),
        },
        RAW.as_bytes(),
        CF_SECRET,
        NOW,
        MAX,
    );
    assert_eq!(missing.unwrap_err(), InboundError::Unauthorized);

    let old = cloudflare_headers(
        RAW.as_bytes(),
        "team@orbit.example",
        NOW - 6 * 60,
        CF_SECRET,
    );
    assert_eq!(
        cloudflare(&old, RAW.as_bytes(), MAX).unwrap_err(),
        InboundError::Replay
    );
}

#[test]
fn cloudflare_refuses_a_message_over_the_limit() {
    let big = format!("{RAW}{}", "x".repeat(2000));
    let headers = cloudflare_headers(big.as_bytes(), "team@orbit.example", NOW, CF_SECRET);
    assert_eq!(
        cloudflare(&headers, big.as_bytes(), 1000).unwrap_err(),
        InboundError::TooLarge
    );
}

#[test]
fn cloudflare_worker_script_signs_the_way_orbit_checks() {
    let script = cloudflare::WORKER_SCRIPT;
    for part in [
        "env.ORBIT_WEBHOOK_URL",
        "env.ORBIT_SIGNING_SECRET",
        "x-orbit-timestamp",
        "x-orbit-signature",
        "x-orbit-from",
        "x-orbit-to",
        "SHA-256",
        "setReject",
    ] {
        assert!(script.contains(part), "{part}");
    }
    // the raw message goes as the body: no base64 or JSON copies of a 25 MiB message
    assert!(!script.contains("btoa"));
}

// ---------------------------------------------------------------- Postmark

fn postmark_body(raw: Option<&str>) -> String {
    let mut body = serde_json::json!({
        "From": "ann@sender.example",
        "OriginalRecipient": "bcc-only@orbit.example",
        "To": "team@orbit.example",
    });
    if let Some(raw) = raw {
        body["RawEmail"] = raw.into();
    }
    body.to_string()
}

fn basic(user: &str, password: &str) -> String {
    format!("Basic {}", base64(format!("{user}:{password}").as_bytes()))
}

#[test]
fn postmark_delivery_needs_its_credentials() {
    let body = postmark_body(Some(RAW));
    let delivery = postmark::verify_and_parse(
        Some(&basic("orbit-user", "pass-word")),
        body.as_bytes(),
        "orbit-user",
        "pass-word",
        MAX,
    )
    .unwrap();
    assert_eq!(delivery.raw, RAW.as_bytes());
    assert_eq!(delivery.recipients, ["bcc-only@orbit.example"]);
    assert_eq!(delivery.spf, SpfSource::Header(SpfHeaderOwned::ReceivedSpf));
    // Postmark gives no envelope sender; the header From is not one
    assert_eq!(delivery.mail_from, None);

    for authorization in [
        None,
        Some(basic("orbit-user", "wrong").as_str()),
        Some("Bearer x"),
    ] {
        let refused = postmark::verify_and_parse(
            authorization,
            body.as_bytes(),
            "orbit-user",
            "pass-word",
            MAX,
        );
        assert_eq!(refused.unwrap_err(), InboundError::Unauthorized);
    }
}

#[test]
fn postmark_without_raw_email_explains_the_setting() {
    let body = postmark_body(None);
    let refused =
        postmark::verify_and_parse(Some(&basic("u", "p")), body.as_bytes(), "u", "p", MAX);
    assert_eq!(
        refused.unwrap_err(),
        InboundError::Malformed("raw_email_missing")
    );
    let big = postmark_body(Some(&format!("{RAW}{}", "x".repeat(2000))));
    let refused =
        postmark::verify_and_parse(Some(&basic("u", "p")), big.as_bytes(), "u", "p", 1000);
    assert_eq!(refused.unwrap_err(), InboundError::TooLarge);
}

// ---------------------------------------------------------------- Mailgun

const MG_KEY: &[u8] = b"mailgun-webhook-signing-key";

fn mailgun_form(timestamp: i64, token: &str, key: &[u8], raw: &str) -> Vec<(String, Vec<u8>)> {
    let timestamp = timestamp.to_string();
    let signature = hmac_hex(key, format!("{timestamp}{token}").as_bytes());
    [
        ("timestamp", timestamp.as_str()),
        ("token", token),
        ("signature", signature.as_str()),
        ("recipient", "bcc-only@orbit.example"),
        ("sender", "ann@sender.example"),
        ("from", "Ann <ann@sender.example>"),
        ("body-mime", raw),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.as_bytes().to_vec()))
    .collect()
}

#[test]
fn mailgun_form_keeps_8bit_bytes() {
    // a Latin-1 body: percent-encoded bytes that are not UTF-8 must come back as they were
    let form = mailgun::parse_urlencoded(b"token=t+1&body-mime=caf%E9%0D%0A&empty=");
    assert_eq!(form[0], ("token".to_owned(), b"t 1".to_vec()));
    assert_eq!(form[1], ("body-mime".to_owned(), b"caf\xe9\r\n".to_vec()));
    assert_eq!(form[2], ("empty".to_owned(), Vec::new()));
}

#[test]
fn mailgun_delivery_returns_its_token() {
    let form = mailgun_form(NOW, "token-1", MG_KEY, RAW);
    let (delivery, token) = mailgun::verify_and_parse(&form, MG_KEY, NOW, MAX).unwrap();
    assert_delivery(&delivery, &["bcc-only@orbit.example"]);
    assert_eq!(token, "token-1");
}

#[test]
fn mailgun_refuses_a_wrong_key_an_old_timestamp_and_a_big_message() {
    let wrong = mailgun_form(NOW, "token-1", b"another-key", RAW);
    assert_eq!(
        mailgun::verify_and_parse(&wrong, MG_KEY, NOW, MAX).unwrap_err(),
        InboundError::Unauthorized
    );
    // Mailgun retries a 503 for 8 hours with the same signature: only older requests are refused
    let retried = mailgun_form(NOW - 7 * 60 * 60, "token-1", MG_KEY, RAW);
    assert!(mailgun::verify_and_parse(&retried, MG_KEY, NOW, MAX).is_ok());
    let old = mailgun_form(NOW - 10 * 60 * 60, "token-1", MG_KEY, RAW);
    assert_eq!(
        mailgun::verify_and_parse(&old, MG_KEY, NOW, MAX).unwrap_err(),
        InboundError::Replay
    );
    let big = mailgun_form(
        NOW,
        "token-1",
        MG_KEY,
        &format!("{RAW}{}", "x".repeat(2000)),
    );
    assert_eq!(
        mailgun::verify_and_parse(&big, MG_KEY, NOW, 1000).unwrap_err(),
        InboundError::TooLarge
    );
}

// ---------------------------------------------------------------- Resend (Svix)

const SVIX_KEY: &[u8] = b"resend-svix-key-0123456789abcdef";

fn svix_secret() -> String {
    format!("whsec_{}", base64(SVIX_KEY))
}

fn svix_signature(id: &str, timestamp: i64, body: &str, key: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(format!("{id}.{timestamp}.{body}").as_bytes());
    format!("v1,{}", base64(&mac.finalize().into_bytes()))
}

const RESEND_EVENT: &str = r#"{"type":"email.received","created_at":"2026-10-28T10:00:00.000Z","data":{"email_id":"56761188-7520-42d8-8898-ff6fc54ce618","from":"ann@sender.example","to":["team@orbit.example"],"bcc":[],"cc":[],"received_for":["bcc-only@orbit.example"],"subject":"Hi"}}"#;

#[test]
fn resend_webhook_gives_the_email_id_and_envelope_recipients() {
    let signature = format!(
        "v1,bm90LXRoaXMtb25l {}",
        svix_signature("msg_1", NOW, RESEND_EVENT, SVIX_KEY)
    );
    let event = resend::verify_webhook(
        Some("msg_1"),
        Some(&NOW.to_string()),
        Some(&signature),
        RESEND_EVENT.as_bytes(),
        &svix_secret(),
        NOW,
    )
    .unwrap();
    assert_eq!(event.email_id, "56761188-7520-42d8-8898-ff6fc54ce618");
    assert_eq!(event.recipients, ["bcc-only@orbit.example"]);
    assert_eq!(event.mail_from.as_deref(), Some("ann@sender.example"));
}

#[test]
fn resend_events_of_other_types_are_recognized_without_their_data() {
    let event = r#"{"type":"domain.updated","data":{"id":"d_1"}}"#;
    let signature = svix_signature("msg_2", NOW, event, SVIX_KEY);
    assert_eq!(
        resend::verify_webhook(
            Some("msg_2"),
            Some(&NOW.to_string()),
            Some(&signature),
            event.as_bytes(),
            &svix_secret(),
            NOW
        )
        .unwrap_err(),
        InboundError::Malformed("event_type")
    );
}

#[test]
fn resend_without_envelope_recipients_is_refused() {
    // the headers' To and Cc are not the envelope (Bcc, lists): no `received_for`, no delivery
    let event = RESEND_EVENT.replace(r#""received_for":["bcc-only@orbit.example"],"#, "");
    let signature = svix_signature("msg_1", NOW, &event, SVIX_KEY);
    assert_eq!(
        resend::verify_webhook(
            Some("msg_1"),
            Some(&NOW.to_string()),
            Some(&signature),
            event.as_bytes(),
            &svix_secret(),
            NOW
        )
        .unwrap_err(),
        InboundError::Malformed("no_envelope")
    );
}

#[test]
fn resend_refuses_a_wrong_signature_and_an_old_timestamp() {
    let wrong = svix_signature("msg_1", NOW, RESEND_EVENT, b"another-key");
    assert_eq!(
        resend::verify_webhook(
            Some("msg_1"),
            Some(&NOW.to_string()),
            Some(&wrong),
            RESEND_EVENT.as_bytes(),
            &svix_secret(),
            NOW
        )
        .unwrap_err(),
        InboundError::Unauthorized
    );
    let old = NOW - 6 * 60;
    let signature = svix_signature("msg_1", old, RESEND_EVENT, SVIX_KEY);
    assert_eq!(
        resend::verify_webhook(
            Some("msg_1"),
            Some(&old.to_string()),
            Some(&signature),
            RESEND_EVENT.as_bytes(),
            &svix_secret(),
            NOW
        )
        .unwrap_err(),
        InboundError::Replay
    );
}

// ---------------------------------------------------------------- Amazon SES (SNS)

/// An RSA certificate like SNS's and its key (made once: RSA key generation is slow in debug).
fn sns_signer() -> &'static (String, rcgen::KeyPair) {
    static SIGNER: OnceLock<(String, rcgen::KeyPair)> = OnceLock::new();
    SIGNER.get_or_init(|| {
        let key = orbit_mail::generate_dkim_key();
        let pair = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(
            &rustls::pki_types::PrivatePkcs8KeyDer::from(key.private_pkcs8_der.clone()),
            &rcgen::PKCS_RSA_SHA256,
        )
        .unwrap();
        let cert = rcgen::CertificateParams::new(vec!["sns.amazonaws.com".to_owned()])
            .unwrap()
            .self_signed(&pair)
            .unwrap();
        (cert.pem(), pair)
    })
}

fn ses_notification() -> String {
    serde_json::json!({
        "notificationType": "Received",
        "mail": { "source": "ann@sender.example", "messageId": "o1" },
        "receipt": {
            "recipients": ["bcc-only@orbit.example"],
            "spfVerdict": { "status": "PASS" },
            "action": { "type": "S3", "bucketName": "orbit-mail", "objectKey": "inbound/o1" }
        }
    })
    .to_string()
}

fn signed_sns(fields: &[(&str, String)], kind: &str) -> String {
    let mut text = String::new();
    for (name, value) in fields {
        text.push_str(&format!("{name}\n{value}\n"));
    }
    let signature = sns_signer().1.sign(text.as_bytes()).unwrap();
    let mut body = serde_json::json!({
        "Type": kind,
        "SignatureVersion": "2",
        "Signature": base64(&signature),
        "SigningCertURL": "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-abc.pem",
    });
    for (name, value) in fields {
        body[*name] = value.clone().into();
    }
    body.to_string()
}

fn notification_fields(message: String) -> Vec<(&'static str, String)> {
    vec![
        ("Message", message),
        ("MessageId", "m-1".to_owned()),
        ("Timestamp", "2026-10-28T10:00:00.000Z".to_owned()),
        (
            "TopicArn",
            "arn:aws:sns:us-east-1:123456789012:orbit".to_owned(),
        ),
        ("Type", "Notification".to_owned()),
    ]
}

#[test]
fn ses_notification_verifies_and_names_the_s3_object() {
    let body = signed_sns(&notification_fields(ses_notification()), "Notification");
    let message = ses::parse_sns(body.as_bytes()).unwrap();
    ses::verify_sns(&message, sns_signer().0.as_bytes()).unwrap();
    let ses::SnsMessage::Notification { ses: receipt, .. } = &message else {
        panic!("not a notification: {message:?}");
    };
    assert_eq!(receipt.bucket, "orbit-mail");
    assert_eq!(receipt.key, "inbound/o1");
    assert_eq!(receipt.recipients, ["bcc-only@orbit.example"]);
    assert_eq!(receipt.mail_from.as_deref(), Some("ann@sender.example"));
    assert_eq!(
        receipt.spf,
        ProviderSpf {
            verdict: Verdict::Pass,
            domain: "sender.example".to_owned()
        }
    );
}

#[test]
fn ses_notifications_older_than_two_hours_are_refused() {
    let mut fields = notification_fields(ses_notification());
    fields[2].1 = "2026-10-28T10:00:00.000Z".to_owned();
    let body = signed_sns(&fields, "Notification");
    let message = ses::parse_sns(body.as_bytes()).unwrap();
    // 2026-10-28T10:00:00Z is 1_793_181_600
    assert!(ses::fresh(&message, 1_793_181_600 + 60 * 60).is_ok());
    assert_eq!(
        ses::fresh(&message, 1_793_181_600 + 3 * 60 * 60).unwrap_err(),
        InboundError::Replay
    );
    let mut bad = notification_fields(ses_notification());
    bad[2].1 = "yesterday".to_owned();
    let message = ses::parse_sns(signed_sns(&bad, "Notification").as_bytes()).unwrap();
    assert_eq!(
        ses::fresh(&message, 1_793_181_600).unwrap_err(),
        InboundError::Replay
    );
}

#[test]
fn ses_signature_breaks_on_one_changed_byte() {
    let body = signed_sns(&notification_fields(ses_notification()), "Notification")
        .replace("inbound/o1", "inbound/o2");
    let message = ses::parse_sns(body.as_bytes()).unwrap();
    assert_eq!(
        ses::verify_sns(&message, sns_signer().0.as_bytes()).unwrap_err(),
        InboundError::Unauthorized
    );
}

#[test]
fn ses_subscription_confirmation_is_signed_too() {
    let fields = vec![
        ("Message", "You have chosen to subscribe".to_owned()),
        ("MessageId", "m-2".to_owned()),
        (
            "SubscribeURL",
            "https://sns.us-east-1.amazonaws.com/?Action=ConfirmSubscription&Token=t".to_owned(),
        ),
        ("Timestamp", "2026-10-28T10:00:00.000Z".to_owned()),
        ("Token", "t".to_owned()),
        (
            "TopicArn",
            "arn:aws:sns:us-east-1:123456789012:orbit".to_owned(),
        ),
        ("Type", "SubscriptionConfirmation".to_owned()),
    ];
    let body = signed_sns(&fields, "SubscriptionConfirmation");
    let message = ses::parse_sns(body.as_bytes()).unwrap();
    ses::verify_sns(&message, sns_signer().0.as_bytes()).unwrap();
    let ses::SnsMessage::SubscriptionConfirmation { subscribe_url, .. } = &message else {
        panic!("not a confirmation: {message:?}");
    };
    assert!(ses::sns_url_ok(subscribe_url));
}

#[test]
fn only_sns_certificate_urls_are_fetched() {
    assert!(ses::cert_url_ok(
        "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-abc.pem"
    ));
    for url in [
        "https://sns.us-east-1.amazonaws.com.evil.example/x.pem",
        "http://sns.us-east-1.amazonaws.com/x.pem",
        "https://evil.example/sns.us-east-1.amazonaws.com/x.pem",
        "https://user@sns.us-east-1.amazonaws.com/x.pem",
        "https://sns.us-east-1.amazonaws.com:8443/x.pem",
        "https://sns.us-east-1.amazonaws.com/x.txt",
        "https://s3.us-east-1.amazonaws.com/x.pem",
    ] {
        assert!(!ses::cert_url_ok(url), "{url}");
    }
}
