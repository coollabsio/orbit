use std::fmt::Write as _;
use std::time::{Duration, Instant};

use orbit_mail::{
    Address, AddressError, ParseError, ParseLimits, base_local, normalize_address,
    normalize_domain, normalize_subject, parse, part_bytes, sniff_image,
};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn parse_fixture(name: &str) -> orbit_mail::ParsedMessage {
    parse(&fixture(name), &ParseLimits::default())
        .unwrap_or_else(|error| panic!("parse {name}: {error:?}"))
}

// ---------------------------------------------------------------- addresses

#[test]
fn address_is_lowercased_without_trailing_dot() {
    assert_eq!(
        normalize_address("Ann.Example@Example.COM.").unwrap(),
        Address {
            local: "ann.example".into(),
            domain: "example.com".into(),
        }
    );
}

#[test]
fn address_domain_becomes_an_a_label() {
    let address = normalize_address("ann@Bücher.example").unwrap();
    assert_eq!(address.domain, "xn--bcher-kva.example");
    assert_eq!(
        normalize_domain("BÜCHER.example.").unwrap(),
        "xn--bcher-kva.example"
    );
    assert_eq!(
        normalize_domain("xn--bcher-kva.example").unwrap(),
        "xn--bcher-kva.example"
    );
}

#[test]
fn relay_address_forms_are_refused() {
    for input in [
        "a@b@c",
        "\"q\"@x",
        "\"a@b\"@x.example",
        "@r:a@b",
        "@r1,@r2:a@b.example",
        "a%b@c",
        "a!b@c",
        "b!a@c.example",
        "",
        "@b.example",
        "a@",
        "a",
        "a b@c.example",
        "a@c .example",
        "a@[192.0.2.1]",
        ".a@c.example",
        "a.@c.example",
        "a..b@c.example",
        "a@-c.example",
        "a@c..example",
        "a@c_d.example",
        "jörg@c.example",
    ] {
        assert!(
            normalize_address(input).is_err(),
            "{input:?} must be refused"
        );
    }
}

#[test]
fn address_length_is_capped() {
    let local = "a".repeat(64);
    let domain = format!(
        "{}.{}.{}.example",
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(50)
    );
    let address = format!("{local}@{domain}");
    assert!(address.len() <= 254);
    assert!(normalize_address(&address).is_ok());

    let long = format!("{local}@{}.{domain}", "e".repeat(20));
    assert!(long.len() > 254);
    assert_eq!(normalize_address(&long), Err(AddressError::TooLong));

    let long_local = format!("{}@c.example", "a".repeat(65));
    assert!(normalize_address(&long_local).is_err());
    let long_label = format!("a@{}.example", "b".repeat(64));
    assert!(normalize_address(&long_label).is_err());
}

#[test]
fn domains_are_validated() {
    for input in [
        "",
        ".",
        "a..b",
        "-a.example",
        "a-.example",
        "a_b.example",
        "[192.0.2.1]",
        "a b.example",
        "a@b.example",
    ] {
        assert!(
            normalize_domain(input).is_err(),
            "{input:?} must be refused"
        );
    }
    assert_eq!(
        normalize_domain("Mail.Example.org").unwrap(),
        "mail.example.org"
    );
}

#[test]
fn base_local_removes_the_tag() {
    assert_eq!(base_local("ann+news"), "ann");
    assert_eq!(base_local("ann+news+more"), "ann");
    assert_eq!(base_local("ann"), "ann");
    assert_eq!(base_local("+news"), "+news");
}

// ---------------------------------------------------------------- subjects

#[test]
fn reply_and_forward_prefixes_are_removed() {
    assert_eq!(
        normalize_subject("Re: Fwd: RE:  Quarterly   numbers"),
        "quarterly numbers"
    );
    assert_eq!(normalize_subject("AW: SV: Fw: hi"), "hi");
    assert_eq!(normalize_subject("Re[2]: hi"), "hi");
    assert_eq!(normalize_subject("re : hi"), "hi");
    assert_eq!(normalize_subject("Re: [list] hello"), "[list] hello");
    assert_eq!(normalize_subject("[list] Re: hello"), "[list] hello");
    assert_eq!(normalize_subject("Reply all"), "reply all");
    assert_eq!(normalize_subject("Re:"), "");
    assert_eq!(normalize_subject(""), "");
    assert_eq!(normalize_subject("  Grüße\taus   Köln "), "grüße aus köln");
}

// ---------------------------------------------------------------- parsing

#[test]
fn plain_message_headers_and_body() {
    let message = parse_fixture("plain.eml");
    assert_eq!(
        message.message_id.as_deref(),
        Some("plain-1@sender.example")
    );
    assert_eq!(message.subject, "Re: Fwd: RE:  Quarterly   numbers");
    assert_eq!(message.normalized_subject, "quarterly numbers");
    assert_eq!(message.sent_at, Some(1_792_654_200));

    let from = message.from.as_ref().unwrap();
    assert_eq!(from.name.as_deref(), Some("Ann Example"));
    assert_eq!(from.address, "ann@sender.example");
    assert_eq!(
        message
            .sender
            .as_ref()
            .map(|sender| sender.address.as_str()),
        Some("list-bot@sender.example")
    );
    assert_eq!(message.reply_to.len(), 1);
    assert_eq!(message.reply_to[0].name.as_deref(), Some("Replies"));

    let to: Vec<_> = message.to.iter().map(|m| m.address.as_str()).collect();
    assert_eq!(to, ["bob@orbit.example", "carol@orbit.example"]);
    assert_eq!(message.to[0].name.as_deref(), Some("Bob"));
    assert_eq!(message.to[1].name, None);
    let cc: Vec<_> = message.cc.iter().map(|m| m.address.as_str()).collect();
    assert_eq!(cc, ["dave@orbit.example", "eve@orbit.example"]);

    assert!(
        message
            .body_text
            .contains("Here are the   quarterly numbers.")
    );
    assert_eq!(
        message.snippet,
        "Hello Bob, Here are the quarterly numbers. Thanks, Ann"
    );
    assert_eq!(message.html_part, None);
    assert!(message.attachments.is_empty());
    assert!(message.in_reply_to.is_empty());
    assert!(message.references.is_empty());
}

#[test]
fn multipart_alternative_has_text_and_html() {
    let message = parse_fixture("multipart_alt.eml");
    assert_eq!(message.body_text.trim(), "Plain version of the reply.");
    let html_part = message.html_part.expect("html part");
    let html = part_bytes(&fixture("multipart_alt.eml"), html_part).unwrap();
    assert!(String::from_utf8(html).unwrap().contains("<b>version</b>"));
    assert_eq!(message.in_reply_to, ["plain-1@sender.example"]);
    assert_eq!(
        message.references,
        ["root@sender.example", "plain-1@sender.example"]
    );
    assert!(message.attachments.is_empty());
    assert_eq!(message.snippet, "Plain version of the reply.");
}

#[test]
fn html_only_message_gets_text_from_html() {
    let raw = b"From: a@sender.example\r\nTo: b@orbit.example\r\nSubject: html\r\n\
        Content-Type: text/html; charset=utf-8\r\n\r\n\
        <html><head><style>p{color:red}</style></head><body><p>Hello <b>there</b></p>\
        <p>second   line</p></body></html>\r\n";
    let message = parse(raw, &ParseLimits::default()).unwrap();
    assert!(message.html_part.is_some());
    assert!(
        message.body_text.contains("Hello there"),
        "{:?}",
        message.body_text
    );
    assert!(!message.snippet.contains('<'));
    assert!(!message.snippet.contains("color"));
    assert!(
        message.snippet.starts_with("Hello there"),
        "{:?}",
        message.snippet
    );
}

#[test]
fn nested_related_lists_inline_image_and_attachment() {
    let raw = fixture("nested_related.eml");
    let message = parse(&raw, &ParseLimits::default()).unwrap();
    assert!(message.html_part.is_some());
    assert_eq!(message.body_text.trim(), "See the logo below.");
    assert_eq!(message.attachments.len(), 2, "{:?}", message.attachments);

    let logo = &message.attachments[0];
    assert_eq!(logo.content_id.as_deref(), Some("logo@sender.example"));
    assert!(logo.inline);
    assert_eq!(logo.declared_type, "image/png");
    assert_eq!(logo.sniffed_type, Some("image/png".into()));
    let logo_bytes = part_bytes(&raw, logo.part_id).unwrap();
    assert!(logo_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(logo.size, logo_bytes.len() as u64);

    let pdf = &message.attachments[1];
    assert_eq!(pdf.filename.as_deref(), Some("report.pdf"));
    assert_eq!(pdf.declared_type, "application/pdf");
    assert_eq!(pdf.sniffed_type, None);
    assert_eq!(pdf.content_id, None);
    assert!(!pdf.inline);
    assert!(
        part_bytes(&raw, pdf.part_id)
            .unwrap()
            .starts_with(b"%PDF-1.4")
    );

    assert_eq!(part_bytes(&raw, 9_999), None);
}

#[test]
fn iso8859_body_and_headers_are_decoded() {
    let message = parse_fixture("iso8859.eml");
    assert_eq!(message.subject, "Café crème");
    assert_eq!(message.normalized_subject, "café crème");
    assert_eq!(
        message.from.as_ref().and_then(|from| from.name.as_deref()),
        Some("François")
    );
    assert!(
        message
            .body_text
            .contains("Un café crème, s'il vous plaît.")
    );
}

#[test]
fn shift_jis_subject_and_body_are_decoded() {
    let raw = b"From: a@sender.example\r\nTo: b@orbit.example\r\n\
        Subject: =?Shift_JIS?B?k/qWe4zqgsyMj5a8?=\r\n\
        Content-Type: text/plain; charset=Shift_JIS\r\n\
        Content-Transfer-Encoding: base64\r\n\r\nlnuVtoLFgrc=\r\n";
    let message = parse(raw, &ParseLimits::default()).unwrap();
    assert_eq!(message.subject, "日本語の件名");
    assert_eq!(message.body_text.trim(), "本文です");
}

#[test]
fn rfc2047_subject_and_display_names_are_decoded() {
    let message = parse_fixture("rfc2047_subject.eml");
    assert_eq!(message.subject, "Grüße aus Köln — 日本語");
    assert_eq!(
        message.from.as_ref().and_then(|from| from.name.as_deref()),
        Some("José Müller")
    );
    assert_eq!(message.to[0].name.as_deref(), Some("Björn"));
    assert_eq!(message.to[0].address, "bjorn@orbit.example");
}

#[test]
fn broken_boundary_still_gives_text() {
    let message = parse_fixture("broken_boundary.eml");
    assert_eq!(message.subject, "Broken boundary");
    assert!(
        message
            .body_text
            .contains("never uses the declared boundary"),
        "{:?}",
        message.body_text
    );
    assert!(message.attachments.is_empty(), "{:?}", message.attachments);
}

#[test]
fn empty_input_is_not_a_limit_error() {
    let error = parse(b"", &ParseLimits::default()).unwrap_err();
    assert!(!matches!(error, ParseError::Limit(_)), "{error:?}");
}

// ---------------------------------------------------------------- limits

fn parts_bomb(parts: usize) -> Vec<u8> {
    let mut raw = String::from(
        "From: a@sender.example\r\nTo: b@orbit.example\r\nSubject: bomb\r\n\
         Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n",
    );
    for index in 0..parts {
        write!(
            raw,
            "--b\r\nContent-Type: text/plain\r\n\r\npart {index}\r\n"
        )
        .unwrap();
    }
    raw.push_str("--b--\r\n");
    raw.into_bytes()
}

fn depth_bomb(levels: usize) -> Vec<u8> {
    let mut raw =
        String::from("From: a@sender.example\r\nTo: b@orbit.example\r\nSubject: deep\r\n");
    for level in 0..levels {
        write!(
            raw,
            "Content-Type: multipart/mixed; boundary=\"l{level}\"\r\n\r\n--l{level}\r\n"
        )
        .unwrap();
    }
    raw.push_str("Content-Type: text/plain\r\n\r\ndeep text\r\n");
    for level in (0..levels).rev() {
        write!(raw, "--l{level}--\r\n").unwrap();
    }
    raw.into_bytes()
}

#[test]
fn parts_bomb_is_refused() {
    assert_eq!(
        parse(&parts_bomb(600), &ParseLimits::default()).unwrap_err(),
        ParseError::Limit("parts")
    );
    let message = parse(&parts_bomb(100), &ParseLimits::default()).unwrap();
    assert!(message.body_text.contains("part 0"));
}

#[test]
fn depth_bomb_is_refused() {
    assert_eq!(
        parse(&depth_bomb(30), &ParseLimits::default()).unwrap_err(),
        ParseError::Limit("depth")
    );
    let message = parse(&depth_bomb(10), &ParseLimits::default()).unwrap();
    assert!(message.body_text.contains("deep text"));
}

#[test]
fn header_count_and_size_are_capped() {
    let mut many = String::from("From: a@sender.example\r\n");
    for index in 0..1_001 {
        write!(many, "X-Filler-{index}: value\r\n").unwrap();
    }
    many.push_str("\r\nbody\r\n");
    assert_eq!(
        parse(many.as_bytes(), &ParseLimits::default()).unwrap_err(),
        ParseError::Limit("headers")
    );

    let mut large = String::from("From: a@sender.example\r\nX-Big: ");
    for _ in 0..(300 * 1024 / 64) {
        large.push_str(&"x".repeat(62));
        large.push_str("\r\n ");
    }
    large.push_str("end\r\n\r\nbody\r\n");
    assert_eq!(
        parse(large.as_bytes(), &ParseLimits::default()).unwrap_err(),
        ParseError::Limit("header_bytes")
    );
}

#[test]
fn references_with_1000_ids_are_capped_at_100() {
    let mut raw = String::from(
        "From: a@sender.example\r\nTo: b@orbit.example\r\nSubject: Re: long thread\r\nReferences:",
    );
    for index in 0..1_000 {
        write!(raw, " <id-{index}@sender.example>\r\n").unwrap();
    }
    raw.push_str("In-Reply-To: <id-999@sender.example>\r\n\r\nbody\r\n");

    let started = Instant::now();
    let message = parse(raw.as_bytes(), &ParseLimits::default()).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(message.references.len(), 100);
    // The root of the thread and the nearest ancestors are kept.
    assert_eq!(message.references[0], "id-0@sender.example");
    assert_eq!(message.references[1], "id-901@sender.example");
    assert_eq!(message.references[99], "id-999@sender.example");
    assert_eq!(message.in_reply_to, ["id-999@sender.example"]);
    assert!(elapsed < Duration::from_secs(1), "parse took {elapsed:?}");
}

// ---------------------------------------------------------------- images

#[test]
fn sniff_image_knows_only_the_allowed_types() {
    assert_eq!(
        sniff_image(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"),
        Some("image/png")
    );
    assert_eq!(
        sniff_image(b"\xff\xd8\xff\xe0\0\x10JFIF"),
        Some("image/jpeg")
    );
    assert_eq!(sniff_image(b"GIF89a\x01\0\x01\0"), Some("image/gif"));
    assert_eq!(sniff_image(b"GIF87a\x01\0\x01\0"), Some("image/gif"));
    assert_eq!(sniff_image(b"RIFF\x24\0\0\0WEBPVP8 "), Some("image/webp"));
    assert_eq!(
        sniff_image(b"\0\0\0\x1cftypavif\0\0\0\0avifmif1miaf"),
        Some("image/avif")
    );
    assert_eq!(
        sniff_image(b"\0\0\0\x1cftypmif1\0\0\0\0mif1avifmiaf"),
        Some("image/avif")
    );
    assert_eq!(sniff_image(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic"), None);
    assert_eq!(sniff_image(b"RIFF\x24\0\0\0WAVEfmt "), None);
    assert_eq!(
        sniff_image(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"),
        None
    );
    assert_eq!(sniff_image(b"<?xml version=\"1.0\"?><svg/>"), None);
    assert_eq!(sniff_image(b"%PDF-1.4"), None);
    assert_eq!(sniff_image(b""), None);
}
