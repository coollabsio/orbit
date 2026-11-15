use orbit_mail::{VacationInput, vacation_allowed};

fn headers(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut all = vec![
        ("From".to_owned(), "Ann <ann@example.org>".to_owned()),
        ("To".to_owned(), "\"Doe, Bob\" <bob@example.com>".to_owned()),
    ];
    all.extend(
        extra
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned())),
    );
    all
}

struct Case {
    name: &'static str,
    mail_from: Option<&'static str>,
    headers: Vec<(String, String)>,
    workspace: bool,
    inbox: bool,
    dmarc_failed: bool,
    replied: bool,
    allowed: bool,
}

fn case(name: &'static str, allowed: bool) -> Case {
    Case {
        name,
        mail_from: Some("ann@example.org"),
        headers: headers(&[]),
        workspace: false,
        inbox: true,
        dmarc_failed: false,
        replied: false,
        allowed,
    }
}

#[test]
fn vacation_rules() {
    let cases = vec![
        case("base case", true),
        Case {
            mail_from: Some("<>"),
            ..case("null sender", false)
        },
        Case {
            mail_from: Some(""),
            ..case("empty sender", false)
        },
        Case {
            mail_from: None,
            ..case("no sender", false)
        },
        Case {
            headers: headers(&[("Auto-Submitted", "auto-replied")]),
            ..case("auto-replied", false)
        },
        Case {
            headers: headers(&[("auto-submitted", "auto-generated")]),
            ..case("auto-generated, lowercase name", false)
        },
        Case {
            headers: headers(&[("Auto-Submitted", "no")]),
            ..case("auto-submitted no", true)
        },
        Case {
            headers: headers(&[("Precedence", "bulk")]),
            ..case("bulk", false)
        },
        Case {
            headers: headers(&[("Precedence", "List")]),
            ..case("list", false)
        },
        Case {
            headers: headers(&[("Precedence", "junk")]),
            ..case("junk", false)
        },
        Case {
            headers: headers(&[("Precedence", "first-class")]),
            ..case("other precedence", true)
        },
        Case {
            headers: headers(&[("List-Id", "<x.example.org>")]),
            ..case("list-id", false)
        },
        Case {
            headers: headers(&[("List-Unsubscribe", "<mailto:u@example.org>")]),
            ..case("list-unsubscribe", false)
        },
        Case {
            headers: vec![
                ("From".into(), "ann@example.org".into()),
                ("To".into(), "team@example.org".into()),
                ("Bcc".into(), "bob@example.com".into()),
            ],
            ..case("bcc only", false)
        },
        Case {
            headers: vec![
                ("From".into(), "ann@example.org".into()),
                ("To".into(), "team@example.org".into()),
                ("CC".into(), "Bob <BOB@Example.com>".into()),
            ],
            ..case("in cc, different case", true)
        },
        Case {
            workspace: true,
            ..case("workspace sender", false)
        },
        Case {
            inbox: false,
            ..case("spam", false)
        },
        Case {
            dmarc_failed: true,
            ..case("dmarc fail", false)
        },
        Case {
            replied: true,
            ..case("replied within 7 days", false)
        },
    ];
    let mailbox = vec!["bob@example.com".to_owned()];
    for case in cases {
        let input = VacationInput {
            mail_from: case.mail_from,
            headers: &case.headers,
            mailbox_addresses: &mailbox,
            sender_is_workspace_mailbox: case.workspace,
            in_inbox: case.inbox,
            dmarc_failed: case.dmarc_failed,
            replied_within_7_days: case.replied,
        };
        assert_eq!(vacation_allowed(&input), case.allowed, "{}", case.name);
    }
}
