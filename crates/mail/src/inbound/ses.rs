//! Amazon SES receiving: a receipt rule stores the message in S3 and publishes a notification to an
//! SNS topic with an HTTPS subscription to Orbit. Orbit verifies the SNS signature with the certificate
//! at `SigningCertURL` (which must be an SNS host), confirms the subscription, and reads the S3 object.
//! <https://docs.aws.amazon.com/sns/latest/dg/sns-verify-signature-of-message-verify-message-signature.html>
//! <https://docs.aws.amazon.com/ses/latest/dg/receiving-email-notifications-contents.html>

use base64::Engine as _;
use ring::signature::{
    RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey,
};
use serde::Deserialize;

use super::{InboundError, address, recipients};
use crate::auth::{ProviderSpf, Verdict};

/// A verified-to-be SNS message (call [`verify_sns`] before using it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnsMessage {
    SubscriptionConfirmation {
        subscribe_url: String,
        topic_arn: String,
        signed: Signed,
    },
    Notification {
        ses: SesReceipt,
        topic_arn: String,
        signed: Signed,
    },
}

/// What the SNS signature covers, and the signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    pub text: String,
    pub signature: Vec<u8>,
    pub version: String,
    pub cert_url: String,
    /// The SNS `Timestamp`, as sent (RFC 3339, UTC).
    pub timestamp: String,
}

/// An SES `Received` notification with an S3 action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SesReceipt {
    pub bucket: String,
    pub key: String,
    /// The envelope recipients that the receipt rule matched.
    pub recipients: Vec<String>,
    pub mail_from: Option<String>,
    pub spf: ProviderSpf,
}

impl SnsMessage {
    #[must_use]
    pub fn signed(&self) -> &Signed {
        match self {
            Self::SubscriptionConfirmation { signed, .. } | Self::Notification { signed, .. } => {
                signed
            }
        }
    }

    #[must_use]
    pub fn topic_arn(&self) -> &str {
        match self {
            Self::SubscriptionConfirmation { topic_arn, .. }
            | Self::Notification { topic_arn, .. } => topic_arn,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Envelope {
    #[serde(rename = "Type")]
    kind: String,
    message: String,
    message_id: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(rename = "SubscribeURL", default)]
    subscribe_url: Option<String>,
    #[serde(default)]
    token: Option<String>,
    timestamp: String,
    topic_arn: String,
    signature: String,
    signature_version: String,
    #[serde(rename = "SigningCertURL")]
    signing_cert_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Notification {
    notification_type: String,
    mail: Mail,
    receipt: Receipt,
}

#[derive(Deserialize)]
struct Mail {
    source: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    #[serde(default)]
    recipients: Vec<String>,
    spf_verdict: Option<Status>,
    action: Action,
}

#[derive(Deserialize)]
struct Status {
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Action {
    #[serde(rename = "type")]
    kind: String,
    bucket_name: Option<String>,
    object_key: Option<String>,
}

/// How old an SNS message may be: SNS retries an HTTP endpoint for about an hour by default.
pub const SNS_WINDOW_SECS: i64 = 2 * 60 * 60;

/// The SNS `Timestamp` of `message` is within [`SNS_WINDOW_SECS`] of `now_secs`, so a captured
/// request cannot be sent again later. Check it after the signature, which covers the timestamp.
pub fn fresh(message: &SnsMessage, now_secs: i64) -> Result<(), InboundError> {
    let sent = rfc3339_secs(&message.signed().timestamp).ok_or(InboundError::Replay)?;
    if (now_secs - sent).abs() > SNS_WINDOW_SECS {
        Err(InboundError::Replay)
    } else {
        Ok(())
    }
}

/// Seconds since 1970 of a UTC timestamp `YYYY-MM-DDTHH:MM:SS[.fff]Z`.
fn rfc3339_secs(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || !value.ends_with('Z') || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if bytes[10] != b'T' || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let number = |from: usize, to: usize| -> Option<i64> {
        let part = value.get(from..to)?;
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let fraction = &value[19..value.len() - 1];
    if !(fraction.is_empty()
        || (fraction.starts_with('.') && fraction[1..].bytes().all(|byte| byte.is_ascii_digit())))
    {
        return None;
    }
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // days from 1970-01-01 (Howard Hinnant's days_from_civil)
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Reads an SNS message (not yet verified).
pub fn parse_sns(body: &[u8]) -> Result<SnsMessage, InboundError> {
    let envelope: Envelope =
        serde_json::from_slice(body).map_err(|_| InboundError::Malformed("json"))?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(envelope.signature.as_bytes())
        .map_err(|_| InboundError::Unauthorized)?;
    let mut text = String::new();
    let mut add = |name: &str, value: &str| {
        text.push_str(name);
        text.push('\n');
        text.push_str(value);
        text.push('\n');
    };
    match envelope.kind.as_str() {
        "Notification" => {
            add("Message", &envelope.message);
            add("MessageId", &envelope.message_id);
            if let Some(subject) = &envelope.subject {
                add("Subject", subject);
            }
            add("Timestamp", &envelope.timestamp);
            add("TopicArn", &envelope.topic_arn);
            add("Type", &envelope.kind);
        }
        "SubscriptionConfirmation" => {
            add("Message", &envelope.message);
            add("MessageId", &envelope.message_id);
            add(
                "SubscribeURL",
                envelope.subscribe_url.as_deref().unwrap_or_default(),
            );
            add("Timestamp", &envelope.timestamp);
            add("Token", envelope.token.as_deref().unwrap_or_default());
            add("TopicArn", &envelope.topic_arn);
            add("Type", &envelope.kind);
        }
        _ => return Err(InboundError::Malformed("sns_type")),
    }
    let signed = Signed {
        text,
        signature,
        version: envelope.signature_version,
        cert_url: envelope.signing_cert_url,
        timestamp: envelope.timestamp,
    };
    match envelope.kind.as_str() {
        "SubscriptionConfirmation" => Ok(SnsMessage::SubscriptionConfirmation {
            subscribe_url: envelope
                .subscribe_url
                .ok_or(InboundError::Malformed("subscribe_url"))?,
            topic_arn: envelope.topic_arn,
            signed,
        }),
        _ => Ok(SnsMessage::Notification {
            ses: ses_receipt(&envelope.message)?,
            topic_arn: envelope.topic_arn,
            signed,
        }),
    }
}

fn ses_receipt(message: &str) -> Result<SesReceipt, InboundError> {
    let notification: Notification =
        serde_json::from_str(message).map_err(|_| InboundError::Malformed("ses_notification"))?;
    if notification.notification_type != "Received" || notification.receipt.action.kind != "S3" {
        return Err(InboundError::Malformed("ses_s3_action"));
    }
    let action = notification.receipt.action;
    let mail_from = address(notification.mail.source.as_deref());
    let verdict = match notification.receipt.spf_verdict.map(|status| status.status) {
        Some(status) if status == "PASS" => Verdict::Pass,
        Some(status) if status == "FAIL" => Verdict::Fail,
        Some(status) if status == "PROCESSING_FAILED" => Verdict::TempError,
        // GRAY is none, softfail or neutral: none of them can pass DMARC
        _ => Verdict::None,
    };
    let domain = mail_from
        .as_deref()
        .and_then(|sender| sender.rsplit_once('@'))
        .map(|(_, domain)| domain.to_ascii_lowercase())
        .unwrap_or_default();
    let envelope: Vec<&str> = notification
        .receipt
        .recipients
        .iter()
        .map(String::as_str)
        .collect();
    Ok(SesReceipt {
        bucket: action
            .bucket_name
            .ok_or(InboundError::Malformed("bucket"))?,
        key: action
            .object_key
            .ok_or(InboundError::Malformed("object_key"))?,
        recipients: recipients(&envelope),
        mail_from,
        spf: ProviderSpf { verdict, domain },
    })
}

/// Verifies the message's signature with the PEM certificate from its `SigningCertURL` (the caller
/// fetched it after [`cert_url_ok`]). Version 1 is RSA with SHA-1, version 2 with SHA-256.
pub fn verify_sns(message: &SnsMessage, cert_pem: &[u8]) -> Result<(), InboundError> {
    let signed = message.signed();
    let (_, pem) =
        x509_parser::pem::parse_x509_pem(cert_pem).map_err(|_| InboundError::Unauthorized)?;
    let cert = pem.parse_x509().map_err(|_| InboundError::Unauthorized)?;
    let public_key = &cert.public_key().subject_public_key.data;
    let algorithm: &dyn ring::signature::VerificationAlgorithm = match signed.version.as_str() {
        "1" => &RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY,
        "2" => &RSA_PKCS1_2048_8192_SHA256,
        _ => return Err(InboundError::Unauthorized),
    };
    UnparsedPublicKey::new(algorithm, public_key)
        .verify(signed.text.as_bytes(), &signed.signature)
        .map_err(|_| InboundError::Unauthorized)
}

/// `https://sns.<region>.amazonaws.com/…` with no user, port, query or fragment.
#[must_use]
pub fn sns_url_ok(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let Some(region) = host
        .strip_prefix("sns.")
        .and_then(|host| host.strip_suffix(".amazonaws.com"))
    else {
        return false;
    };
    !region.is_empty()
        && region
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && rest[host.len()..].starts_with('/')
}

/// An SNS signing certificate URL: an [`sns_url_ok`] URL to a `.pem` file, with no query.
#[must_use]
pub fn cert_url_ok(url: &str) -> bool {
    sns_url_ok(url) && !url.contains(['?', '#']) && url.ends_with(".pem")
}
