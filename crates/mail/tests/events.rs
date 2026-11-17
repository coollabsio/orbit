//! Send route events (spec §5.5, §5.7): each provider's request is authenticated first, then read
//! into events. The payloads follow each provider's documented examples:
//! - PostShiba: <https://www.postshiba.com/docs/webhooks/event-types.md>
//! - Mailgun: <https://documentation.mailgun.com/docs/mailgun/user-manual/webhooks/webhook-payloads>
//! - Resend: <https://resend.com/docs/webhooks/emails/bounced>
//! - Postmark: <https://postmarkapp.com/developer/webhooks/bounce-webhook>,
//!   <https://postmarkapp.com/developer/webhooks/spam-complaint-webhook>
//! - SES: <https://docs.aws.amazon.com/ses/latest/dg/notification-contents.html>
//! - Cloudflare: <https://developers.cloudflare.com/email-service/reference/event-subscriptions/>

use std::sync::OnceLock;

use base64::Engine as _;
use hmac::{Hmac, Mac};
use orbit_mail::events::{
    EventError, EventKind, EventSecret, ParsedEvent, ProviderEvent, SnsEvents, sns_cert_url,
    verify_and_parse, verify_sns_events,
};
use orbit_mail::inbound::cloudflare;
use rcgen::SigningKey as _;
use sha2::Sha256;

/// 2026-10-27T08:53:20Z.
const NOW: i64 = 1_793_091_200;

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

fn headers(pairs: &[(&str, String)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn hmac(secret: &str) -> EventSecret {
    EventSecret::Hmac(secret.to_owned())
}

// ---------------------------------------------------------------- PostShiba

const POSTSHIBA_SECRET: &str = "postshiba-endpoint-secret";

/// The bounce of the docs' example shape, with Orbit's correlation in `unique_args`.
fn postshiba_body() -> String {
    serde_json::json!([{
        "event": "bounce",
        "email": "you@example.com",
        "timestamp": NOW - 30,
        "sg_event_id": "capsule-45",
        "sg_message_id": "abc@capsule.test",
        "smtp-id": "<abc@capsule.test>",
        "response": "550 5.1.1 The email account that you tried to reach does not exist",
        "reason": "550 5.1.1 The email account that you tried to reach does not exist",
        "status": "5.1.1",
        "type": "bounce",
        "tenant_id": "default",
        "from": "hello@mail.example.com",
        "unique_args": { "orbit": "0190d0c4-7e9a-7b3c-9f12-1a2b3c4d5e6f" },
        "orbit": "0190d0c4-7e9a-7b3c-9f12-1a2b3c4d5e6f"
    }])
    .to_string()
}

fn postshiba_headers(body: &str, timestamp: i64, secret: &str) -> Vec<(String, String)> {
    let signature = hmac_hex(secret.as_bytes(), format!("{timestamp}.{body}").as_bytes());
    headers(&[
        ("X-Capsule-Timestamp", timestamp.to_string()),
        ("X-Capsule-Signature", format!("sha256={signature}")),
    ])
}

#[test]
fn postshiba_valid_signature_parses() {
    let body = postshiba_body();
    let events = verify_and_parse(
        EventKind::Postshiba,
        &postshiba_headers(&body, NOW, POSTSHIBA_SECRET),
        body.as_bytes(),
        &hmac(POSTSHIBA_SECRET),
        NOW,
    )
    .unwrap();
    assert_eq!(
        events,
        [ParsedEvent {
            provider_message_id: Some("abc@capsule.test".to_owned()),
            correlation: Some("0190d0c4-7e9a-7b3c-9f12-1a2b3c4d5e6f".to_owned()),
            message_id: Some("abc@capsule.test".to_owned()),
            recipient: "you@example.com".to_owned(),
            at_ms: Some((NOW - 30) * 1000),
            event: ProviderEvent::Bounced {
                hard: true,
                diagnostic: Some(
                    "550 5.1.1 The email account that you tried to reach does not exist".to_owned()
                ),
            },
        }]
    );
}

#[test]
fn postshiba_bad_signature_rejected() {
    let body = postshiba_body();
    let wrong = postshiba_headers(&body, NOW, "another-secret");
    assert_eq!(
        verify_and_parse(
            EventKind::Postshiba,
            &wrong,
            body.as_bytes(),
            &hmac(POSTSHIBA_SECRET),
            NOW
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
    // a changed body
    let signed = postshiba_headers(&body, NOW, POSTSHIBA_SECRET);
    let changed = body.replace("you@example.com", "me@example.com");
    assert_eq!(
        verify_and_parse(
            EventKind::Postshiba,
            &signed,
            changed.as_bytes(),
            &hmac(POSTSHIBA_SECRET),
            NOW
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
    assert_eq!(
        verify_and_parse(
            EventKind::Postshiba,
            &[],
            body.as_bytes(),
            &hmac(POSTSHIBA_SECRET),
            NOW
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
}

#[test]
fn postshiba_blocked_complaint_and_other_events() {
    let body = serde_json::json!([
        { "event": "bounce", "type": "blocked", "email": "a@example.com", "timestamp": NOW,
          "response": "554 Message rejected due to policy" },
        { "event": "spamreport", "email": "b@example.com", "timestamp": NOW },
        { "event": "deferred", "email": "c@example.com", "timestamp": NOW,
          "response": "451 4.2.0 Try later" },
        { "event": "processed", "email": "d@example.com", "timestamp": NOW },
        { "event": "delivered", "email": "e@example.com", "timestamp": NOW,
          "response": "250 2.0.0 OK" }
    ])
    .to_string();
    let events = verify_and_parse(
        EventKind::Postshiba,
        &postshiba_headers(&body, NOW, POSTSHIBA_SECRET),
        body.as_bytes(),
        &hmac(POSTSHIBA_SECRET),
        NOW,
    )
    .unwrap();
    let kinds: Vec<(&str, &ProviderEvent)> = events
        .iter()
        .map(|event| (event.recipient.as_str(), &event.event))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                "a@example.com",
                &ProviderEvent::Bounced {
                    hard: true,
                    diagnostic: Some("5.7.1 554 Message rejected due to policy".to_owned()),
                }
            ),
            ("b@example.com", &ProviderEvent::Complained),
            (
                "c@example.com",
                &ProviderEvent::Deferred {
                    diagnostic: Some("451 4.2.0 Try later".to_owned())
                }
            ),
            ("e@example.com", &ProviderEvent::Delivered),
        ]
    );
}

#[test]
fn old_timestamp_rejected() {
    // PostShiba signs each attempt anew: 5 minutes
    let body = postshiba_body();
    let old = NOW - 6 * 60;
    assert_eq!(
        verify_and_parse(
            EventKind::Postshiba,
            &postshiba_headers(&body, old, POSTSHIBA_SECRET),
            body.as_bytes(),
            &hmac(POSTSHIBA_SECRET),
            NOW
        )
        .unwrap_err(),
        EventError::Replay
    );
    // Mailgun
    let body = mailgun_body(NOW - 6 * 60, MAILGUN_KEY, &mailgun_failed());
    assert_eq!(
        verify_and_parse(
            EventKind::Mailgun,
            &[],
            body.as_bytes(),
            &hmac(MAILGUN_KEY),
            NOW
        )
        .unwrap_err(),
        EventError::Replay
    );
    // Resend (Svix)
    let old = NOW - 6 * 60;
    let body = resend_bounced();
    assert_eq!(
        verify_and_parse(
            EventKind::Resend,
            &svix_headers("msg_1", old, &body, SVIX_KEY),
            body.as_bytes(),
            &hmac(&svix_secret()),
            NOW
        )
        .unwrap_err(),
        EventError::Replay
    );
    // Cloudflare's Worker
    let body = cloudflare_body();
    assert_eq!(
        verify_and_parse(
            EventKind::Cloudflare,
            &cloudflare_headers(&body, old, CF_SECRET),
            body.as_bytes(),
            &hmac(CF_SECRET),
            NOW
        )
        .unwrap_err(),
        EventError::Replay
    );
    // SNS keeps its signing time across retries: two hours
    let fields = ses_fields(ses_bounce(), "2026-10-27T05:00:00.000Z");
    let body = signed_sns(&fields, "Notification");
    assert_eq!(
        verify_sns_events(body.as_bytes(), TOPIC, sns_signer().0.as_bytes(), NOW).unwrap_err(),
        EventError::Replay
    );
}

// ---------------------------------------------------------------- Mailgun

const MAILGUN_KEY: &str = "key-mailgun-webhook-signing";

/// The docs' "Permanent Failed Event", with Orbit's `user-variables`.
fn mailgun_failed() -> serde_json::Value {
    serde_json::json!({
        "event": "failed",
        "id": "2kFItcrLQuKTdp-Ia2Xr7w",
        "timestamp": 1_793_091_170.592_369_3,
        "message": {
            "attachments": [],
            "headers": {
                "message-id": "20260212174255.58bbb7ce85a423e5@mailgun.com",
                "from": "Sample Sender <sender@sample.mailgun.com>",
                "to": "badrecipient@sample.mailgun.com",
                "subject": "Sample webhook payload"
            },
            "size": 384
        },
        "log-level": "error",
        "recipient": "badrecipient@sample.mailgun.com",
        "recipient-domain": "sample.mailgun.com",
        "reason": "bounce",
        "severity": "permanent",
        "delivery-status": {
            "attempt-no": 1,
            "code": 550,
            "message": "5.5.0 Requested action not taken: mailbox unavailable (S2017062302).",
            "description": null,
            "enhanced-code": "5.5.0",
            "bounce-type": "hard"
        },
        "user-variables": { "orbit": "0190d0c4-7e9a-7b3c-9f12-1a2b3c4d5e6f" }
    })
}

fn mailgun_body(timestamp: i64, key: &str, data: &serde_json::Value) -> String {
    let token = "a8ce0edb2dd8301dee6c2405235584e45aa91d1e9f979f3de0";
    serde_json::json!({
        "signature": {
            "timestamp": timestamp.to_string(),
            "token": token,
            "signature": hmac_hex(key.as_bytes(), format!("{timestamp}{token}").as_bytes()),
        },
        "event-data": data,
    })
    .to_string()
}

#[test]
fn mailgun_valid_signature_parses() {
    let body = mailgun_body(NOW, MAILGUN_KEY, &mailgun_failed());
    let events = verify_and_parse(
        EventKind::Mailgun,
        &[],
        body.as_bytes(),
        &hmac(MAILGUN_KEY),
        NOW,
    )
    .unwrap();
    assert_eq!(
        events,
        [ParsedEvent {
            provider_message_id: Some("20260212174255.58bbb7ce85a423e5@mailgun.com".to_owned()),
            correlation: Some("0190d0c4-7e9a-7b3c-9f12-1a2b3c4d5e6f".to_owned()),
            message_id: Some("20260212174255.58bbb7ce85a423e5@mailgun.com".to_owned()),
            recipient: "badrecipient@sample.mailgun.com".to_owned(),
            at_ms: Some(1_793_091_170_592),
            event: ProviderEvent::Bounced {
                hard: true,
                diagnostic: Some(
                    "550 5.5.0 Requested action not taken: mailbox unavailable (S2017062302)."
                        .to_owned()
                ),
            },
        }]
    );
    // a temporary failure, with no user variables (`[]`)
    let mut data = mailgun_failed();
    data["severity"] = "temporary".into();
    data["user-variables"] = serde_json::json!([]);
    let body = mailgun_body(NOW, MAILGUN_KEY, &data);
    let events = verify_and_parse(
        EventKind::Mailgun,
        &[],
        body.as_bytes(),
        &hmac(MAILGUN_KEY),
        NOW,
    )
    .unwrap();
    assert!(matches!(events[0].event, ProviderEvent::Deferred { .. }));
    assert_eq!(events[0].correlation, None);
    // a complaint; an open is not an event Orbit uses
    for (kind, expected) in [("complained", 1), ("opened", 0)] {
        let mut data = mailgun_failed();
        data["event"] = kind.into();
        let body = mailgun_body(NOW, MAILGUN_KEY, &data);
        let events = verify_and_parse(
            EventKind::Mailgun,
            &[],
            body.as_bytes(),
            &hmac(MAILGUN_KEY),
            NOW,
        )
        .unwrap();
        assert_eq!(events.len(), expected, "{kind}");
    }
}

#[test]
fn mailgun_bad_signature_rejected() {
    let body = mailgun_body(NOW, "another-key", &mailgun_failed());
    assert_eq!(
        verify_and_parse(
            EventKind::Mailgun,
            &[],
            body.as_bytes(),
            &hmac(MAILGUN_KEY),
            NOW
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
    // the signature covers only timestamp and token: no signature object, no event
    assert_eq!(
        verify_and_parse(
            EventKind::Mailgun,
            &[],
            br#"{"event-data":{}}"#,
            &hmac(MAILGUN_KEY),
            NOW
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
}

// ---------------------------------------------------------------- Resend (Svix)

const SVIX_KEY: &[u8] = b"resend-svix-key-0123456789abcdef";

fn svix_secret() -> String {
    format!("whsec_{}", base64(SVIX_KEY))
}

fn svix_headers(id: &str, timestamp: i64, body: &str, key: &[u8]) -> Vec<(String, String)> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(format!("{id}.{timestamp}.{body}").as_bytes());
    headers(&[
        ("svix-id", id.to_owned()),
        ("svix-timestamp", timestamp.to_string()),
        (
            "svix-signature",
            format!("v1,{}", base64(&mac.finalize().into_bytes())),
        ),
    ])
}

/// The docs' `email.bounced` example.
fn resend_bounced() -> String {
    serde_json::json!({
        "type": "email.bounced",
        "created_at": "2026-10-27T08:52:12.126Z",
        "data": {
            "broadcast_id": "8b146471-e88e-4322-86af-016cd36fd216",
            "created_at": "2026-10-27T08:52:11.894719+00:00",
            "email_id": "56761188-7520-42d8-8898-ff6fc54ce618",
            "message_id": "<0190d0c4.abc@one.test>",
            "from": "Acme <onboarding@resend.dev>",
            "to": ["delivered@resend.dev"],
            "subject": "Sending this example",
            "bounce": {
                "diagnosticCode": ["smtp; 550 5.1.1 user unknown"],
                "message": "The recipient's email address is on the suppression list because it has a recent history of producing hard bounces.",
                "subType": "General",
                "type": "Permanent"
            },
            "tags": { "category": "confirm_email" }
        }
    })
    .to_string()
}

#[test]
fn resend_valid_signature_parses() {
    let body = resend_bounced();
    let events = verify_and_parse(
        EventKind::Resend,
        &svix_headers("msg_1", NOW, &body, SVIX_KEY),
        body.as_bytes(),
        &hmac(&svix_secret()),
        NOW,
    )
    .unwrap();
    assert_eq!(
        events,
        [ParsedEvent {
            provider_message_id: Some("56761188-7520-42d8-8898-ff6fc54ce618".to_owned()),
            correlation: None,
            message_id: Some("0190d0c4.abc@one.test".to_owned()),
            recipient: "delivered@resend.dev".to_owned(),
            at_ms: Some(1_793_091_132_126),
            event: ProviderEvent::Bounced {
                hard: true,
                diagnostic: Some("550 5.1.1 user unknown".to_owned()),
            },
        }]
    );
    // a complaint, a delay, and an event Orbit does not use
    for (kind, expected) in [
        ("email.complained", Some(ProviderEvent::Complained)),
        (
            "email.delivery_delayed",
            Some(ProviderEvent::Deferred { diagnostic: None }),
        ),
        ("email.opened", None),
    ] {
        let body = body.replace("email.bounced", kind);
        let events = verify_and_parse(
            EventKind::Resend,
            &svix_headers("msg_2", NOW, &body, SVIX_KEY),
            body.as_bytes(),
            &hmac(&svix_secret()),
            NOW,
        )
        .unwrap();
        assert_eq!(events.first().map(|event| event.event.clone()), expected);
    }
}

#[test]
fn resend_bad_signature_rejected() {
    let body = resend_bounced();
    assert_eq!(
        verify_and_parse(
            EventKind::Resend,
            &svix_headers("msg_1", NOW, &body, b"another-key"),
            body.as_bytes(),
            &hmac(&svix_secret()),
            NOW
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
    // signed for another message ID
    let mut signed = svix_headers("msg_1", NOW, &body, SVIX_KEY);
    signed[0].1 = "msg_2".to_owned();
    assert_eq!(
        verify_and_parse(
            EventKind::Resend,
            &signed,
            body.as_bytes(),
            &hmac(&svix_secret()),
            NOW
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
}

// ---------------------------------------------------------------- Postmark

fn basic(user: &str, password: &str) -> Vec<(String, String)> {
    headers(&[(
        "Authorization",
        format!("Basic {}", base64(format!("{user}:{password}").as_bytes())),
    )])
}

fn postmark_secret() -> EventSecret {
    EventSecret::Basic {
        user: "orbit".to_owned(),
        password: "s3cret-webhook".to_owned(),
    }
}

/// The docs' bounce webhook example, with Orbit's metadata.
const POSTMARK_BOUNCE: &str = r#"{
  "RecordType": "Bounce",
  "MessageStream": "outbound",
  "ID": 4323372036854775807,
  "Type": "HardBounce",
  "TypeCode": 1,
  "Name": "Hard bounce",
  "Tag": "Test",
  "MessageID": "883953f4-6105-42a2-a16a-77a8eac79483",
  "Metadata": {"orbit": "0190d0c4-7e9a-7b3c-9f12-1a2b3c4d5e6f"},
  "ServerID": 23,
  "Description": "The server was unable to deliver your message (ex: unknown user, mailbox not found).",
  "Details": "Test bounce details",
  "Email": "john@example.com",
  "From": "sender@example.com",
  "BouncedAt": "2019-11-05T16:33:54.9070259Z",
  "DumpAvailable": true,
  "Inactive": true,
  "CanActivate": true,
  "Subject": "Test subject",
  "Content": "Test content"
}"#;

#[test]
fn postmark_valid_signature_parses() {
    let events = verify_and_parse(
        EventKind::Postmark,
        &basic("orbit", "s3cret-webhook"),
        POSTMARK_BOUNCE.as_bytes(),
        &postmark_secret(),
        NOW,
    )
    .unwrap();
    assert_eq!(
        events,
        [ParsedEvent {
            provider_message_id: Some("883953f4-6105-42a2-a16a-77a8eac79483".to_owned()),
            correlation: Some("0190d0c4-7e9a-7b3c-9f12-1a2b3c4d5e6f".to_owned()),
            message_id: None,
            recipient: "john@example.com".to_owned(),
            at_ms: Some(1_572_971_634_907),
            event: ProviderEvent::Bounced {
                hard: true,
                diagnostic: Some("Test bounce details".to_owned()),
            },
        }]
    );
    // the spam complaint webhook
    let complaint = POSTMARK_BOUNCE
        .replace(
            r#""RecordType": "Bounce""#,
            r#""RecordType": "SpamComplaint""#,
        )
        .replace(r#""TypeCode": 1"#, r#""TypeCode": 100001"#);
    let events = verify_and_parse(
        EventKind::Postmark,
        &basic("orbit", "s3cret-webhook"),
        complaint.as_bytes(),
        &postmark_secret(),
        NOW,
    )
    .unwrap();
    assert_eq!(events[0].event, ProviderEvent::Complained);
    // an ISP block bounces, as a policy refusal; an auto reply changes nothing
    for (code, expected) in [
        (
            100_006,
            Some(ProviderEvent::Bounced {
                hard: true,
                diagnostic: Some("5.7.1 Test bounce details".to_owned()),
            }),
        ),
        (64, None),
    ] {
        let body = POSTMARK_BOUNCE.replace(r#""TypeCode": 1"#, &format!(r#""TypeCode": {code}"#));
        let events = verify_and_parse(
            EventKind::Postmark,
            &basic("orbit", "s3cret-webhook"),
            body.as_bytes(),
            &postmark_secret(),
            NOW,
        )
        .unwrap();
        assert_eq!(events.first().map(|event| event.event.clone()), expected);
    }
}

#[test]
fn postmark_bad_signature_rejected() {
    for wrong in [
        basic("orbit", "guess"),
        basic("other", "s3cret-webhook"),
        Vec::new(),
    ] {
        assert_eq!(
            verify_and_parse(
                EventKind::Postmark,
                &wrong,
                POSTMARK_BOUNCE.as_bytes(),
                &postmark_secret(),
                NOW
            )
            .unwrap_err(),
            EventError::Unauthorized
        );
    }
}

// ---------------------------------------------------------------- Amazon SES (SNS)

const TOPIC: &str = "arn:aws:sns:us-east-1:123456789012:orbit-events";

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

/// The docs' bounce notification example.
fn ses_bounce() -> String {
    serde_json::json!({
        "notificationType": "Bounce",
        "bounce": {
            "feedbackId": "0100015e6ac11a54-d4f7f2b3-3a1e-4f7c-a5d4-2e7c1e8e7a5c-000000",
            "bounceType": "Permanent",
            "bounceSubType": "General",
            "bouncedRecipients": [{
                "emailAddress": "recipient@example.com",
                "action": "failed",
                "status": "5.1.1",
                "diagnosticCode": "smtp; 550 5.1.1 user unknown"
            }],
            "timestamp": "2026-10-27T08:52:00.000Z",
            "remoteMtaIp": "127.0.2.0",
            "reportingMTA": "dsn; a8-70.smtp-out.amazonses.com"
        },
        "mail": {
            "timestamp": "2026-10-27T08:51:58.000Z",
            "source": "john@example.com",
            "sendingAccountId": "123456789012",
            "messageId": "00000138111222aa-33322211-cccc-cccc-cccc-ddddaaaa0680-000000",
            "destination": ["recipient@example.com"],
            "commonHeaders": { "messageId": "<0190d0c4.abc@one.test>" }
        }
    })
    .to_string()
}

fn ses_fields(message: String, timestamp: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Message", message),
        ("MessageId", "m-1".to_owned()),
        ("Timestamp", timestamp.to_owned()),
        ("TopicArn", TOPIC.to_owned()),
        ("Type", "Notification".to_owned()),
    ]
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

#[test]
fn ses_valid_signature_parses() {
    let body = signed_sns(
        &ses_fields(ses_bounce(), "2026-10-27T08:52:01.000Z"),
        "Notification",
    );
    assert_eq!(
        sns_cert_url(body.as_bytes(), TOPIC).unwrap(),
        "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-abc.pem"
    );
    let SnsEvents::Events(events) =
        verify_sns_events(body.as_bytes(), TOPIC, sns_signer().0.as_bytes(), NOW).unwrap()
    else {
        panic!("events");
    };
    assert_eq!(
        events,
        [ParsedEvent {
            provider_message_id: Some(
                "00000138111222aa-33322211-cccc-cccc-cccc-ddddaaaa0680-000000".to_owned()
            ),
            correlation: None,
            message_id: Some("0190d0c4.abc@one.test".to_owned()),
            recipient: "recipient@example.com".to_owned(),
            at_ms: Some(1_793_091_120_000),
            event: ProviderEvent::Bounced {
                hard: true,
                diagnostic: Some("550 5.1.1 user unknown".to_owned()),
            },
        }]
    );
    // the same through the common entry point
    let events = verify_and_parse(
        EventKind::Ses,
        &[],
        body.as_bytes(),
        &EventSecret::Sns {
            topic_arn: TOPIC.to_owned(),
            cert_pem: sns_signer().0.as_bytes().to_vec(),
        },
        NOW,
    )
    .unwrap();
    assert_eq!(events.len(), 1);
    // a complaint (configuration set event publishing names it `eventType`)
    let complaint = serde_json::json!({
        "eventType": "Complaint",
        "complaint": {
            "complainedRecipients": [{ "emailAddress": "recipient@example.com" }],
            "timestamp": "2026-10-27T08:52:00.000Z",
            "complaintFeedbackType": "abuse"
        },
        "mail": { "messageId": "00000138111222aa-33322211-cccc-cccc-cccc-ddddaaaa0680-000000" }
    })
    .to_string();
    let body = signed_sns(
        &ses_fields(complaint, "2026-10-27T08:52:01.000Z"),
        "Notification",
    );
    let SnsEvents::Events(events) =
        verify_sns_events(body.as_bytes(), TOPIC, sns_signer().0.as_bytes(), NOW).unwrap()
    else {
        panic!("events");
    };
    assert_eq!(events[0].event, ProviderEvent::Complained);
}

#[test]
fn ses_subscription_confirmation_is_returned_after_the_signature() {
    let fields = vec![
        ("Message", "You have chosen to subscribe".to_owned()),
        ("MessageId", "m-2".to_owned()),
        (
            "SubscribeURL",
            "https://sns.us-east-1.amazonaws.com/?Action=ConfirmSubscription&Token=t".to_owned(),
        ),
        ("Timestamp", "2026-10-27T08:52:01.000Z".to_owned()),
        ("Token", "t".to_owned()),
        ("TopicArn", TOPIC.to_owned()),
        ("Type", "SubscriptionConfirmation".to_owned()),
    ];
    let body = signed_sns(&fields, "SubscriptionConfirmation");
    assert_eq!(
        verify_sns_events(body.as_bytes(), TOPIC, sns_signer().0.as_bytes(), NOW).unwrap(),
        SnsEvents::Confirm {
            subscribe_url:
                "https://sns.us-east-1.amazonaws.com/?Action=ConfirmSubscription&Token=t".to_owned()
        }
    );
}

#[test]
fn ses_bad_signature_rejected() {
    let body = signed_sns(
        &ses_fields(ses_bounce(), "2026-10-27T08:52:01.000Z"),
        "Notification",
    );
    // one changed byte of the message
    let changed = body.replace("recipient@example.com", "recipiunt@example.com");
    assert_eq!(
        verify_sns_events(changed.as_bytes(), TOPIC, sns_signer().0.as_bytes(), NOW).unwrap_err(),
        EventError::Unauthorized
    );
    // another topic: SNS signs every topic's messages; its certificate is not even fetched
    assert_eq!(
        sns_cert_url(
            body.as_bytes(),
            "arn:aws:sns:us-east-1:999999999999:attacker"
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
    assert_eq!(
        verify_sns_events(
            body.as_bytes(),
            "arn:aws:sns:us-east-1:999999999999:attacker",
            sns_signer().0.as_bytes(),
            NOW
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
    // a certificate that is not on an SNS host is not fetched
    let elsewhere = body.replace(
        "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-abc.pem",
        "https://evil.example/cert.pem",
    );
    assert_eq!(
        sns_cert_url(elsewhere.as_bytes(), TOPIC).unwrap_err(),
        EventError::Unauthorized
    );
}

// ---------------------------------------------------------------- Cloudflare (Worker)

const CF_SECRET: &str = "cloudflare-event-secret";

/// A batch the Worker posts: the docs' `message.bounced` example.
fn cloudflare_body() -> String {
    serde_json::json!([{
        "type": "cf.email.sending.message.bounced",
        "source": { "type": "email.sending", "zoneId": "023e105f4ecef8ad9ca31a8372d0c353",
                    "domain": "send.example.com" },
        "payload": {
            "eventId": "0190d0c4-7ea1-7af2-8b88-c1d2e3f4a5b6",
            "messageId": "0101018f7d0c4d9a-msg-bounced",
            "sender": "receipts@send.example.com",
            "recipient": "user@example.net",
            "subject": "Your receipt",
            "terminal": true,
            "delivery": {
                "status": "bounced",
                "provider": "external_smtp",
                "smtpStatusCode": "550",
                "smtpEnhancedStatusCode": "5.1.1",
                "smtpResponse": "550 5.1.1 User unknown"
            },
            "bounce": { "type": "hard", "classification": "permanent_failure",
                        "reason": "550 5.1.1 User unknown" }
        },
        "metadata": { "accountId": "f9f79265f388666de8122cfb508d7776",
                      "eventSchemaVersion": 1, "eventTimestamp": "2026-10-27T08:52:00.132Z" }
    }, {
        "type": "cf.email.sending.message.complained",
        "payload": { "messageId": "0101018f7d0c4d9a-msg-complained",
                     "recipient": "user2@example.net", "complaint": { "type": "abuse" } },
        "metadata": { "eventTimestamp": "2026-10-27T08:52:00.132Z" }
    }])
    .to_string()
}

fn cloudflare_headers(body: &str, timestamp: i64, secret: &str) -> Vec<(String, String)> {
    headers(&[
        ("x-orbit-timestamp", timestamp.to_string()),
        (
            "x-orbit-signature",
            hmac_hex(secret.as_bytes(), format!("{timestamp}.{body}").as_bytes()),
        ),
    ])
}

#[test]
fn cloudflare_valid_signature_parses() {
    let body = cloudflare_body();
    let events = verify_and_parse(
        EventKind::Cloudflare,
        &cloudflare_headers(&body, NOW, CF_SECRET),
        body.as_bytes(),
        &hmac(CF_SECRET),
        NOW,
    )
    .unwrap();
    assert_eq!(
        events[0],
        ParsedEvent {
            provider_message_id: Some("0101018f7d0c4d9a-msg-bounced".to_owned()),
            correlation: None,
            message_id: None,
            recipient: "user@example.net".to_owned(),
            at_ms: Some(1_793_091_120_132),
            event: ProviderEvent::Bounced {
                hard: true,
                diagnostic: Some("550 5.1.1 User unknown".to_owned()),
            },
        }
    );
    assert_eq!(events[1].event, ProviderEvent::Complained);
    assert_eq!(events[1].recipient, "user2@example.net");
}

#[test]
fn cloudflare_bad_signature_rejected() {
    let body = cloudflare_body();
    for (signed, secret) in [
        (cloudflare_headers(&body, NOW, "another-secret"), CF_SECRET),
        (cloudflare_headers(&body, NOW, CF_SECRET), "rotated-secret"),
        (Vec::new(), CF_SECRET),
    ] {
        assert_eq!(
            verify_and_parse(
                EventKind::Cloudflare,
                &signed,
                body.as_bytes(),
                &hmac(secret),
                NOW
            )
            .unwrap_err(),
            EventError::Unauthorized
        );
    }
}

#[test]
fn cloudflare_worker_script_posts_signed_event_batches() {
    let script = cloudflare::WORKER_SCRIPT;
    for part in [
        "async queue(batch, env)",
        "env.ORBIT_EVENTS_URL",
        "env.ORBIT_EVENT_SECRET || env.ORBIT_SIGNING_SECRET",
        "batch.messages.map((message) => message.body)",
        "timestamp + \".\" + body",
        "x-orbit-timestamp",
        "x-orbit-signature",
        "if (!response.ok) throw",
    ] {
        assert!(script.contains(part), "{part}");
    }
}

#[test]
fn a_secret_of_another_kind_is_refused() {
    let body = postshiba_body();
    assert_eq!(
        verify_and_parse(
            EventKind::Postshiba,
            &postshiba_headers(&body, NOW, POSTSHIBA_SECRET),
            body.as_bytes(),
            &postmark_secret(),
            NOW
        )
        .unwrap_err(),
        EventError::Unauthorized
    );
}
