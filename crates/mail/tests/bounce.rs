use orbit_mail::{DsnAction, bounce_local_part, is_hard, parse_bounce_local_part, parse_dsn};

const KEY: [u8; 32] = [7; 32];
const ID: [u8; 16] = *b"0123456789abcdef";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn token_round_trip() {
    let local = bounce_local_part(&ID, &KEY);
    assert!(local.starts_with("bounce+"));
    assert_eq!(local, local.to_lowercase());
    assert!(!local.contains('='));
    assert_eq!(parse_bounce_local_part(&local, &KEY), Some(ID));
}

#[test]
fn token_rejects_flipped_bit() {
    let local = bounce_local_part(&ID, &KEY);
    let (prefix, token) = local.split_at(7);
    let mut bytes = token.as_bytes().to_vec();
    for index in 0..bytes.len() {
        let original = bytes[index];
        bytes[index] = if original == b'a' { b'b' } else { b'a' };
        let forged = format!("{prefix}{}", String::from_utf8(bytes.clone()).unwrap());
        assert_eq!(
            parse_bounce_local_part(&forged, &KEY),
            None,
            "index {index}"
        );
        bytes[index] = original;
    }
    assert_eq!(parse_bounce_local_part("bounce+", &KEY), None);
    assert_eq!(parse_bounce_local_part("bounce+!!", &KEY), None);
    assert_eq!(parse_bounce_local_part("other+abc", &KEY), None);
}

#[test]
fn token_rejects_other_key() {
    let local = bounce_local_part(&ID, &KEY);
    assert_eq!(parse_bounce_local_part(&local, &[8; 32]), None);
}

#[test]
fn token_case_insensitive() {
    let local = bounce_local_part(&ID, &KEY).to_uppercase();
    assert_eq!(parse_bounce_local_part(&local, &KEY), Some(ID));
}

#[test]
fn parses_hard_bounce() {
    let dsn = parse_dsn(&fixture("dsn_gmail_hard.eml")).unwrap();
    assert_eq!(dsn.len(), 1);
    assert_eq!(dsn[0].address, "nobody@example.org");
    assert_eq!(dsn[0].action, DsnAction::Failed);
    assert_eq!(dsn[0].status, "5.1.1");
    let diagnostic = dsn[0].diagnostic.as_deref().unwrap();
    assert!(
        diagnostic.contains("5.1.1") && diagnostic.contains("does not exist"),
        "{diagnostic}"
    );
    assert!(is_hard(&dsn[0].status));
}

#[test]
fn parses_delayed() {
    let dsn = parse_dsn(&fixture("dsn_delayed.eml")).unwrap();
    assert_eq!(dsn.len(), 1);
    assert_eq!(dsn[0].action, DsnAction::Delayed);
    assert_eq!(dsn[0].status, "4.4.7");
    assert_eq!(dsn[0].diagnostic, None);
    assert!(!is_hard(&dsn[0].status));
}

#[test]
fn parses_postfix_with_several_recipients() {
    let dsn = parse_dsn(&fixture("dsn_postfix.eml")).unwrap();
    assert_eq!(dsn.len(), 2);
    assert_eq!(dsn[0].address, "gone@example.org");
    assert_eq!(dsn[0].action, DsnAction::Failed);
    assert_eq!(dsn[0].status, "5.2.2");
    assert_eq!(dsn[1].action, DsnAction::Delivered);
}

#[test]
fn non_dsn_is_none() {
    assert!(parse_dsn(&fixture("dsn_not_a_dsn.eml")).is_none());
    assert!(parse_dsn(&fixture("plain.eml")).is_none());
}

#[test]
fn reads_original_recipient_and_strips_brackets() {
    let raw = b"From: MAILER-DAEMON@mx.example.org\r\n\
To: bounce+x@one.test\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/report; report-type=delivery-status; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain\r\n\
\r\n\
Failed.\r\n\
--b\r\n\
Content-Type: message/delivery-status\r\n\
\r\n\
Reporting-MTA: dns; mx.example.org\r\n\
\r\n\
Original-Recipient: rfc822;<Ann@Example.org>\r\n\
Final-Recipient: RFC822; <ann.lee@internal.example.org>\r\n\
Action: failed\r\n\
Status: 5.1.1\r\n\
\r\n\
--b--\r\n";
    let dsn = parse_dsn(raw).unwrap();
    assert_eq!(dsn.len(), 1);
    assert_eq!(dsn[0].address, "ann.lee@internal.example.org");
    assert_eq!(dsn[0].original.as_deref(), Some("ann@example.org"));
    let dsn = parse_dsn(&fixture("dsn_gmail_hard.eml")).unwrap();
    assert_eq!(dsn[0].original, None);
}
