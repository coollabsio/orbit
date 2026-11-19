use mail_parser::{MessageParser, MimeHeaders};
use orbit_mail::{
    ComposeError, Composed, ComposedAddress, ComposedFile, ComposedFileKind, ParseLimits,
    build_message, clean_outgoing_html, html_to_text, new_message_id, parse, trim_references,
};

fn addr(name: Option<&str>, address: &str) -> ComposedAddress {
    ComposedAddress {
        name: name.map(str::to_owned),
        address: address.to_owned(),
    }
}

fn base() -> Composed {
    Composed {
        message_id: "<m1@orbit.test>".into(),
        date: 1_794_000_000,
        from: addr(Some("Ann"), "ann@orbit.test"),
        to: vec![addr(None, "bob@example.com")],
        cc: vec![],
        reply_to: None,
        subject: "Hello".into(),
        in_reply_to: None,
        references: vec![],
        html: "<p>Hi <b>there</b></p>".into(),
        text: "Hi there".into(),
        files: vec![],
        extra_headers: vec![],
    }
}

fn file(name: &str, kind: ComposedFileKind) -> ComposedFile {
    ComposedFile {
        filename: name.into(),
        content_type: "image/png".into(),
        bytes: vec![1, 2, 3, 4],
        kind,
    }
}

#[test]
fn builds_alternative_text_and_html() {
    let raw = build_message(&base()).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    assert!(text.contains("MIME-Version: 1.0\r\n"));
    assert!(!text.replace("\r\n", "").contains('\n'));
    let msg = MessageParser::default().parse(&raw).unwrap();
    assert_eq!(msg.text_body_count(), 1);
    assert_eq!(msg.html_body_count(), 1);
    assert_eq!(msg.body_text(0).unwrap().trim(), "Hi there");
    assert!(msg.body_html(0).unwrap().contains("<b>there</b>"));
    assert_eq!(msg.message_id(), Some("m1@orbit.test"));
    assert_eq!(msg.date().unwrap().to_timestamp(), 1_794_000_000);
    let parsed = parse(&raw, &ParseLimits::default()).unwrap();
    assert_eq!(parsed.subject, "Hello");
}

#[test]
fn never_writes_bcc() {
    let raw = build_message(&base()).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("Bcc:"));
}

#[test]
fn reply_headers() {
    let mut c = base();
    c.in_reply_to = Some("<p@x>".into());
    c.references = vec!["<a@x>".into()];
    let raw = build_message(&c).unwrap();
    let parsed = parse(&raw, &ParseLimits::default()).unwrap();
    assert_eq!(parsed.in_reply_to, vec!["p@x".to_owned()]);
    assert_eq!(parsed.references, vec!["a@x".to_owned()]);
}

#[test]
fn trim_references_keeps_first_and_last_19() {
    let refs: Vec<String> = (0..40).map(|i| format!("<r{i}@x>")).collect();
    let out = trim_references(&refs, "<parent@x>");
    assert_eq!(out.len(), 20);
    assert_eq!(out[0], "<r0@x>");
    assert_eq!(out[1], "<r22@x>");
    assert_eq!(out[19], "<parent@x>");
    let short = trim_references(&["<a@x>".to_owned()], "<p@x>");
    assert_eq!(short, vec!["<a@x>".to_owned(), "<p@x>".to_owned()]);
}

#[test]
fn inline_image_makes_related() {
    let mut c = base();
    c.files.push(file(
        "logo.png",
        ComposedFileKind::Inline {
            content_id: "img1".into(),
        },
    ));
    let raw = build_message(&c).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    assert!(text.contains("multipart/related"));
    assert!(text.contains("Content-ID: <img1>"));
    let msg = MessageParser::default().parse(&raw).unwrap();
    assert_eq!(msg.html_body_count(), 1);
    let inline = msg
        .parts
        .iter()
        .find(|p| p.content_id() == Some("img1"))
        .unwrap();
    assert_eq!(inline.contents(), &[1, 2, 3, 4]);
}

#[test]
fn forward_as_attachment_is_rfc822() {
    let mut c = base();
    let mut f = file("Original.eml", ComposedFileKind::Message);
    f.content_type = "message/rfc822".into();
    f.bytes = b"From: a@b.c\r\nSubject: Old\r\n\r\nbody\r\n".to_vec();
    c.files.push(f);
    let raw = build_message(&c).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    assert!(text.contains("Content-Type: message/rfc822"));
    assert!(!text.contains("Content-Transfer-Encoding: base64"));
    assert!(text.contains("Content-Transfer-Encoding: 7bit\r\n"));
    assert!(text.contains("\r\n\r\nFrom: a@b.c\r\nSubject: Old"));
    let msg = MessageParser::default().parse(&raw).unwrap();
    assert!(msg.parts.iter().any(|p| {
        p.content_type()
            .is_some_and(|t| t.c_type == "message" && t.subtype() == Some("rfc822"))
    }));
}

#[test]
fn attachment_round_trips() {
    let mut c = base();
    c.files.push(file("a.png", ComposedFileKind::Attachment));
    let raw = build_message(&c).unwrap();
    let msg = MessageParser::default().parse(&raw).unwrap();
    assert_eq!(msg.attachment_count(), 1);
    assert_eq!(msg.attachment(0).unwrap().contents(), &[1, 2, 3, 4]);
}

#[test]
fn non_ascii_name_subject_filename() {
    let mut c = base();
    c.from = addr(Some("Zoë"), "zoe@orbit.test");
    c.subject = "日本語".into();
    c.files
        .push(file("日本語.pdf", ComposedFileKind::Attachment));
    let raw = build_message(&c).unwrap();
    let msg = MessageParser::default().parse(&raw).unwrap();
    assert_eq!(msg.subject(), Some("日本語"));
    let from = msg.from().unwrap().first().unwrap();
    assert_eq!(from.name(), Some("Zoë"));
    assert_eq!(
        msg.attachment(0).unwrap().attachment_name(),
        Some("日本語.pdf")
    );
}

#[test]
fn rejects_crlf_in_header_values() {
    let mut c = base();
    c.subject = "a\r\nBcc: x@y".into();
    assert!(matches!(
        build_message(&c),
        Err(ComposeError::InvalidHeader("subject"))
    ));
    let mut c = base();
    c.extra_headers.push(("X-A".into(), "v\nBcc: x@y".into()));
    assert!(matches!(
        build_message(&c),
        Err(ComposeError::InvalidHeader("extra_headers"))
    ));
    let mut c = base();
    c.to = vec![addr(Some("a\r\nBcc: x"), "b@c.d")];
    assert!(matches!(
        build_message(&c),
        Err(ComposeError::InvalidHeader("to"))
    ));
}

#[test]
fn bcc_only_names_undisclosed_recipients() {
    let mut c = base();
    c.to.clear();
    c.cc.clear();
    let raw = String::from_utf8(build_message(&c).unwrap()).unwrap();
    assert!(raw.contains("To: undisclosed-recipients:;\r\n"), "{raw}");
}

#[test]
fn extra_headers_are_written() {
    let mut c = base();
    c.extra_headers
        .push(("List-Unsubscribe".into(), "<mailto:u@orbit.test>".into()));
    let raw = build_message(&c).unwrap();
    assert!(String::from_utf8_lossy(&raw).contains("List-Unsubscribe: <mailto:u@orbit.test>\r\n"));
}

#[test]
fn message_id_shape() {
    let id = new_message_id("orbit.test");
    assert!(id.starts_with('<') && id.ends_with("@orbit.test>"));
    assert_ne!(id, new_message_id("orbit.test"));
}

#[test]
fn outgoing_html_keeps_cid_images_only_safe_markup() {
    let clean = clean_outgoing_html(
        "<p onclick=\"x()\" style=\"color:red\">Hi</p><img src=\"cid:logo@orbit\">\
         <script>alert(1)</script><a href=\"javascript:x()\">bad</a><img src=\"https://a.test/i.png\">",
    );
    assert!(clean.contains("src=\"cid:logo@orbit\""), "{clean}");
    assert!(clean.contains("https://a.test/i.png"), "{clean}");
    assert!(
        !clean.contains("onclick") && !clean.contains("style="),
        "{clean}"
    );
    assert!(
        !clean.contains("script") && !clean.contains("javascript"),
        "{clean}"
    );
}

#[test]
fn html_to_text_basic() {
    assert_eq!(
        html_to_text("<p>Hi <b>there</b></p><ul><li>a</li></ul>"),
        "Hi there\n\n- a"
    );
    assert_eq!(
        html_to_text("a<br>b &amp; c&nbsp;&lt;d&gt;"),
        "a\nb & c <d>"
    );
    assert_eq!(html_to_text("<style>x{}</style>Hi"), "Hi");
}

#[test]
fn extra_headers_refuse_reserved_names() {
    for name in ["Bcc", "message-id", "DKIM-Signature", "Return-Path"] {
        let mut c = base();
        c.extra_headers.push((name.into(), "x".into()));
        assert!(
            matches!(
                build_message(&c),
                Err(ComposeError::InvalidHeader("extra_headers"))
            ),
            "{name}"
        );
    }
}

#[test]
fn rejects_malformed_addresses() {
    for bad in ["nobody", "@example.com", "bob@", "a@b@c"] {
        let mut c = base();
        c.to = vec![addr(None, bad)];
        assert!(
            matches!(build_message(&c), Err(ComposeError::InvalidHeader("to"))),
            "{bad}"
        );
    }
}
