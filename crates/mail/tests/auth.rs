use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use base64::Engine as _;
use mail_auth::common::crypto::Ed25519Key;
use mail_auth::common::headers::HeaderWriter as _;
use mail_auth::dkim::DkimSigner;
use orbit_mail::{
    AuthOutcome, Authenticator, DmarcPolicy, DmarcVerdict, ProviderSpf, SpfHeader, StaticDns,
    Verdict, authentication_results, provider_spf, strip_auth_results, trusted_relay_results,
};

const ALLOWED_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
const OTHER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7));
const SPF: &str = "v=spf1 ip4:192.0.2.10 -all";
const DMARC_REJECT: &str = "v=DMARC1; p=reject";

fn message(body: &str) -> String {
    format!(
        "From: Ann <ann@example.com>\r\nTo: support@orbit.test\r\nSubject: Hello\r\n\
         Date: Thu, 22 Oct 2026 10:00:00 +0000\r\nMessage-ID: <1@example.com>\r\n\r\n{body}\r\n"
    )
}

/// Signs `raw` for `example.com` with a fresh Ed25519 key and returns the signed message and the DKIM record.
fn signed(raw: &str) -> (String, String) {
    let pkcs8 = Ed25519Key::generate_pkcs8().unwrap();
    let key = Ed25519Key::from_pkcs8_der(&pkcs8).unwrap();
    let record = format!(
        "v=DKIM1; k=ed25519; p={}",
        base64::engine::general_purpose::STANDARD.encode(key.public_key())
    );
    let signature = DkimSigner::from_key(key)
        .domain("example.com")
        .selector("s1")
        .headers(["From", "To", "Subject", "Date", "Message-ID"])
        .sign(raw.as_bytes())
        .unwrap();
    (format!("{}{raw}", signature.to_header()), record)
}

async fn check(dns: StaticDns, raw: &str, ip: IpAddr, mail_from: Option<&str>) -> AuthOutcome {
    let authenticator = Authenticator::with_static_dns(dns).unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        authenticator.check(raw.as_bytes(), ip, "mail.example.com", mail_from),
    )
    .await
    .expect("the check must not wait on the network")
}

// ---------------------------------------------------------------- SPF

#[tokio::test]
async fn spf_passes_for_an_allowed_ip() {
    let dns = StaticDns::new().txt("example.com", SPF);
    let outcome = check(dns, &message("hi"), ALLOWED_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.spf, Verdict::Pass);
    assert_eq!(outcome.spf_domain.as_deref(), Some("example.com"));
    assert_eq!(outcome.dkim, Verdict::None);
    assert_eq!(outcome.dmarc, DmarcVerdict::None);
    assert!(!outcome.temp_error);
}

#[tokio::test]
async fn spf_fails_for_another_ip() {
    let dns = StaticDns::new().txt("example.com", SPF);
    let outcome = check(dns, &message("hi"), OTHER_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.spf, Verdict::Fail);
    assert!(!outcome.temp_error);
}

#[tokio::test]
async fn null_sender_checks_the_helo_name() {
    let dns = StaticDns::new().txt("mail.example.com", SPF);
    let outcome = check(dns, &message("hi"), ALLOWED_IP, None).await;
    assert_eq!(outcome.spf, Verdict::Pass);
    assert_eq!(outcome.spf_domain.as_deref(), Some("mail.example.com"));
}

#[tokio::test]
async fn spf_follows_preloaded_a_and_mx_records() {
    let dns = StaticDns::new()
        .txt("example.com", "v=spf1 mx a:out.example.com -all")
        .mx("example.com", 10, "mx.example.com")
        .a("mx.example.com", Ipv4Addr::new(192, 0, 2, 10))
        .aaaa("out.example.com", "2001:db8::25".parse().unwrap())
        .a("out.example.com", Ipv4Addr::new(192, 0, 2, 25));
    let mx = check(
        dns.clone(),
        &message("hi"),
        ALLOWED_IP,
        Some("ann@example.com"),
    )
    .await;
    assert_eq!(mx.spf, Verdict::Pass);
    let a = check(
        dns.clone(),
        &message("hi"),
        "192.0.2.25".parse().unwrap(),
        Some("ann@example.com"),
    )
    .await;
    assert_eq!(a.spf, Verdict::Pass);
    let aaaa = check(
        dns,
        &message("hi"),
        "2001:db8::25".parse().unwrap(),
        Some("ann@example.com"),
    )
    .await;
    assert_eq!(aaaa.spf, Verdict::Pass);
}

#[tokio::test]
async fn no_spf_record_is_none() {
    let outcome = check(
        StaticDns::new(),
        &message("hi"),
        ALLOWED_IP,
        Some("ann@example.com"),
    )
    .await;
    assert_eq!(outcome.spf, Verdict::None);
    assert!(!outcome.temp_error);
}

// ---------------------------------------------------------------- DKIM and DMARC

#[tokio::test]
async fn dkim_passes_and_aligns_dmarc() {
    let (raw, record) = signed(&message("hi"));
    let dns = StaticDns::new()
        .txt("example.com", SPF)
        .txt("s1._domainkey.example.com", &record)
        .txt("_dmarc.example.com", DMARC_REJECT);
    // SPF fails (wrong IP); the aligned DKIM signature alone passes DMARC.
    let outcome = check(dns, &raw, OTHER_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.spf, Verdict::Fail);
    assert_eq!(outcome.dkim, Verdict::Pass);
    assert_eq!(outcome.dkim_domain.as_deref(), Some("example.com"));
    assert_eq!(outcome.dmarc, DmarcVerdict::Pass);
    assert_eq!(outcome.from_domain.as_deref(), Some("example.com"));
    assert_eq!(outcome.aligned_domain.as_deref(), Some("example.com"));
    assert!(!outcome.temp_error);
}

#[tokio::test]
async fn dkim_fails_after_a_body_change() {
    let (raw, record) = signed(&message("hi"));
    let raw = raw.replace("hi\r\n", "hi there\r\n");
    let dns = StaticDns::new()
        .txt("example.com", SPF)
        .txt("s1._domainkey.example.com", &record)
        .txt("_dmarc.example.com", DMARC_REJECT);
    let outcome = check(dns, &raw, OTHER_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.dkim, Verdict::Fail);
    assert_eq!(
        outcome.dmarc,
        DmarcVerdict::Fail {
            policy: DmarcPolicy::Reject
        }
    );
    assert_eq!(outcome.aligned_domain, None);
}

#[tokio::test]
async fn dkim_fails_after_a_signed_header_change() {
    let (raw, record) = signed(&message("hi"));
    let raw = raw.replace("Subject: Hello", "Subject: Pay now");
    let dns = StaticDns::new().txt("s1._domainkey.example.com", &record);
    let outcome = check(dns, &raw, OTHER_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.dkim, Verdict::Fail);
    assert_eq!(outcome.dmarc, DmarcVerdict::None);
}

#[tokio::test]
async fn dmarc_reject_fails_without_aligned_pass() {
    let dns = StaticDns::new()
        .txt("example.com", SPF)
        .txt("_dmarc.example.com", DMARC_REJECT);
    let outcome = check(dns, &message("hi"), OTHER_IP, Some("ann@example.com")).await;
    assert_eq!(
        outcome.dmarc,
        DmarcVerdict::Fail {
            policy: DmarcPolicy::Reject
        }
    );
    assert!(!outcome.temp_error);
}

#[tokio::test]
async fn dmarc_spf_pass_from_another_domain_is_not_aligned() {
    let dns = StaticDns::new()
        .txt("bounce.example.net", SPF)
        .txt("_dmarc.example.com", "v=DMARC1; p=quarantine");
    let outcome = check(
        dns,
        &message("hi"),
        ALLOWED_IP,
        Some("b@bounce.example.net"),
    )
    .await;
    assert_eq!(outcome.spf, Verdict::Pass);
    assert_eq!(
        outcome.dmarc,
        DmarcVerdict::Fail {
            policy: DmarcPolicy::Quarantine
        }
    );
}

#[tokio::test]
async fn dmarc_passes_on_aligned_spf() {
    let dns = StaticDns::new()
        .txt("example.com", SPF)
        .txt("_dmarc.example.com", DMARC_REJECT);
    let outcome = check(dns, &message("hi"), ALLOWED_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.dmarc, DmarcVerdict::Pass);
    assert_eq!(outcome.aligned_domain.as_deref(), Some("example.com"));
}

// ---------------------------------------------------------------- temporary errors

#[tokio::test]
async fn dns_timeout_is_a_temporary_error() {
    let dns = StaticDns::new()
        .temp_error("example.com")
        .txt("_dmarc.example.com", DMARC_REJECT);
    let outcome = check(dns, &message("hi"), ALLOWED_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.spf, Verdict::TempError);
    assert_eq!(outcome.dmarc, DmarcVerdict::TempError);
    assert!(outcome.temp_error);
}

#[tokio::test]
async fn dmarc_lookup_timeout_is_a_temporary_error() {
    let dns = StaticDns::new()
        .txt("example.com", SPF)
        .temp_error("_dmarc.example.com");
    let outcome = check(dns, &message("hi"), OTHER_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.dmarc, DmarcVerdict::TempError);
    assert!(outcome.temp_error);
}

#[tokio::test]
async fn unlisted_names_fail_fast_without_the_network() {
    // The `a:` lookup is not preloaded; the static resolver has no name server, so it fails at once.
    let dns = StaticDns::new().txt("example.com", "v=spf1 a:mx.unlisted.example -all");
    let outcome = check(dns, &message("hi"), ALLOWED_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.spf, Verdict::TempError);
    assert!(outcome.temp_error);
}

#[tokio::test]
async fn spf_temp_error_does_not_defer_a_dmarc_pass() {
    let (raw, record) = signed(&message("hi"));
    let dns = StaticDns::new()
        .temp_error("example.com")
        .txt("s1._domainkey.example.com", &record)
        .txt("_dmarc.example.com", DMARC_REJECT);
    let outcome = check(dns, &raw, ALLOWED_IP, Some("ann@example.com")).await;
    assert_eq!(outcome.spf, Verdict::TempError);
    assert_eq!(outcome.dmarc, DmarcVerdict::Pass);
    assert!(!outcome.temp_error);
}

// ---------------------------------------------------------------- Authentication-Results

fn outcome() -> AuthOutcome {
    AuthOutcome {
        spf: Verdict::Pass,
        dkim: Verdict::Fail,
        dmarc: DmarcVerdict::Fail {
            policy: DmarcPolicy::Reject,
        },
        aligned_domain: None,
        temp_error: false,
        spf_domain: Some("example.com".into()),
        dkim_domain: Some("example.com".into()),
        from_domain: Some("example.com".into()),
    }
}

#[test]
fn authentication_results_is_an_rfc8601_value() {
    assert_eq!(
        authentication_results("orbit.example", &outcome()),
        "orbit.example; spf=pass smtp.mailfrom=example.com; dkim=fail header.d=example.com; \
         dmarc=fail policy.dmarc=reject header.from=example.com"
    );
}

#[test]
fn authentication_results_without_domains() {
    let outcome = AuthOutcome {
        spf: Verdict::None,
        dkim: Verdict::None,
        dmarc: DmarcVerdict::None,
        aligned_domain: None,
        temp_error: false,
        spf_domain: None,
        dkim_domain: None,
        from_domain: None,
    };
    assert_eq!(
        authentication_results("orbit.example", &outcome),
        "orbit.example; spf=none; dkim=none; dmarc=none"
    );
}

#[test]
fn authentication_results_drops_unsafe_domains() {
    let mut outcome = outcome();
    outcome.spf_domain = Some("evil.example\r\nX-Injected: yes".into());
    outcome.dkim_domain = Some("a b;c".into());
    let value = authentication_results("orbit.example", &outcome);
    assert!(
        !value.contains('\n') && !value.contains("Injected"),
        "{value}"
    );
    assert!(
        value.starts_with("orbit.example; spf=pass; dkim=fail; dmarc=fail"),
        "{value}"
    );
}

#[test]
fn own_results_read_back_as_the_same_outcome() {
    let mut header = String::from("Authentication-Results: ");
    header.push_str(&authentication_results("relay.example", &outcome()));
    let raw = format!("{header}\r\n{}", message("hi"));
    assert_eq!(
        trusted_relay_results(raw.as_bytes(), "relay.example"),
        Some(outcome())
    );
}

// ---------------------------------------------------------------- stripping and relay trust

#[test]
fn strip_removes_only_headers_with_our_authserv_id() {
    let raw = "Authentication-Results: orbit.example; spf=pass\r\n\
               Received: from x\r\n\
               Authentication-Results: (forged) ORBIT.Example 1;\r\n\tdkim=pass\r\n\
               Authentication-Results: \"orbit.example\"; dmarc=pass\r\n\
               Authentication-Results: mx.google.com; spf=pass\r\n\
               Authentication-Results: orbit.example.evil; spf=pass\r\n\
               X-Authentication-Results: orbit.example; spf=pass\r\n\
               Subject: hi\r\n\
               \r\n\
               Authentication-Results: orbit.example; body text stays\r\n";
    let stripped = strip_auth_results(raw.as_bytes(), "orbit.example");
    assert_eq!(
        String::from_utf8(stripped).unwrap(),
        "Received: from x\r\n\
         Authentication-Results: mx.google.com; spf=pass\r\n\
         Authentication-Results: orbit.example.evil; spf=pass\r\n\
         X-Authentication-Results: orbit.example; spf=pass\r\n\
         Subject: hi\r\n\
         \r\n\
         Authentication-Results: orbit.example; body text stays\r\n"
    );
}

#[test]
fn strip_handles_bare_lf_messages() {
    let raw = "Authentication-Results: orbit.example; spf=pass\nSubject: hi\n\nbody\n";
    assert_eq!(
        strip_auth_results(raw.as_bytes(), "orbit.example"),
        b"Subject: hi\n\nbody\n"
    );
}

#[test]
fn trusted_relay_results_reads_only_the_topmost_relay_header() {
    let raw = "Authentication-Results: mx.other.example; dmarc=pass header.from=example.com\r\n\
               Authentication-Results: mx.google.com;\r\n\
               \tdkim=pass header.i=@example.com header.s=s1 header.b=abc;\r\n\
               \tspf=pass (google.com: domain of ann@example.com designates 192.0.2.10 as permitted sender; ok) \
               smtp.mailfrom=ann@example.com;\r\n\
               \tdmarc=fail (p=REJECT sp=REJECT dis=NONE) header.from=example.com\r\n\
               Authentication-Results: mx.google.com; spf=pass; dkim=pass; dmarc=pass header.from=example.com\r\n\
               Subject: hi\r\n\r\nbody\r\n";
    let outcome = trusted_relay_results(raw.as_bytes(), "mx.google.com").unwrap();
    assert_eq!(outcome.spf, Verdict::Pass);
    assert_eq!(outcome.spf_domain.as_deref(), Some("example.com"));
    assert_eq!(outcome.dkim, Verdict::Pass);
    assert_eq!(outcome.dkim_domain.as_deref(), Some("example.com"));
    assert_eq!(
        outcome.dmarc,
        DmarcVerdict::Fail {
            policy: DmarcPolicy::Reject
        }
    );
    assert_eq!(outcome.from_domain.as_deref(), Some("example.com"));
    assert_eq!(outcome.aligned_domain, None);
    assert!(!outcome.temp_error);
}

#[test]
fn trusted_relay_results_reads_a_temporary_error() {
    let raw = "Authentication-Results: relay.example; spf=temperror smtp.mailfrom=example.com; dkim=none; \
               dmarc=temperror header.from=example.com\r\n\r\nbody\r\n";
    let outcome = trusted_relay_results(raw.as_bytes(), "relay.example").unwrap();
    assert_eq!(outcome.spf, Verdict::TempError);
    assert_eq!(outcome.dmarc, DmarcVerdict::TempError);
    assert!(outcome.temp_error);
}

#[test]
fn trusted_relay_results_reads_none_and_missing() {
    let raw = "Authentication-Results: relay.example; none\r\n\r\nbody\r\n";
    let outcome = trusted_relay_results(raw.as_bytes(), "relay.example").unwrap();
    assert_eq!(outcome.spf, Verdict::None);
    assert_eq!(outcome.dkim, Verdict::None);
    assert_eq!(outcome.dmarc, DmarcVerdict::None);

    let raw = "Authentication-Results: other.example; spf=pass\r\n\r\nAuthentication-Results: relay.example; spf=pass\r\n";
    assert_eq!(trusted_relay_results(raw.as_bytes(), "relay.example"), None);
}

// ---------------------------------------------------------------- the provider's SPF

async fn check_relayed(dns: StaticDns, raw: &str, spf: Option<ProviderSpf>) -> AuthOutcome {
    let authenticator = Authenticator::with_static_dns(dns).unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        authenticator.check_relayed(
            raw.as_bytes(),
            Some("ann@example.com"),
            "provider.example",
            spf,
        ),
    )
    .await
    .expect("the check must not wait on the network")
}

#[tokio::test]
async fn provider_spf_pass_aligns_dmarc_without_dkim() {
    let dns = StaticDns::new().txt("_dmarc.example.com", DMARC_REJECT);
    let spf = ProviderSpf {
        verdict: Verdict::Pass,
        domain: "example.com".to_owned(),
    };
    let outcome = check_relayed(dns, &message("hi"), Some(spf)).await;
    assert_eq!(outcome.spf, Verdict::Pass);
    assert_eq!(outcome.dkim, Verdict::None);
    assert_eq!(outcome.dmarc, DmarcVerdict::Pass);
    assert_eq!(outcome.spf_domain.as_deref(), Some("example.com"));
}

#[tokio::test]
async fn provider_spf_none_and_no_dkim_fails_dmarc() {
    let dns = StaticDns::new().txt("_dmarc.example.com", DMARC_REJECT);
    let outcome = check_relayed(dns, &message("hi"), None).await;
    assert_eq!(outcome.spf, Verdict::None);
    assert_eq!(
        outcome.dmarc,
        DmarcVerdict::Fail {
            policy: DmarcPolicy::Reject
        }
    );
}

#[tokio::test]
async fn dkim_still_checked_by_orbit() {
    let (signed, record) = signed(&message("original"));
    let changed = signed.replace("original", "changed");
    let dns = StaticDns::new()
        .txt("s1._domainkey.example.com", &record)
        .txt("_dmarc.example.com", DMARC_REJECT);
    let outcome = check_relayed(dns, &changed, None).await;
    assert_eq!(outcome.dkim, Verdict::Fail);
}

#[test]
fn only_the_topmost_received_spf_counts() {
    let raw = format!(
        "Received-SPF: fail (provider: domain of ann@example.com does not designate 203.0.113.9) \
         client-ip=203.0.113.9; envelope-from=\"ann@example.com\"; helo=mx.attacker.example;\r\n\
         Received-SPF: pass (forged) envelope-from=\"ann@example.com\";\r\n{}",
        message("hi")
    );
    let spf = provider_spf(raw.as_bytes(), SpfHeader::ReceivedSpf).unwrap();
    assert_eq!(spf.verdict, Verdict::Fail);
    assert_eq!(spf.domain, "example.com");
}

#[test]
fn provider_spf_reads_the_providers_authentication_results() {
    let raw = format!(
        "Authentication-Results: mx.provider.example; spf=pass smtp.mailfrom=ann@example.com\r\n\
         Authentication-Results: mx.provider.example; spf=fail smtp.mailfrom=ann@example.com\r\n{}",
        message("hi")
    );
    let spf = provider_spf(
        raw.as_bytes(),
        SpfHeader::AuthResults {
            authserv_id: "mx.provider.example",
        },
    )
    .unwrap();
    assert_eq!(spf.verdict, Verdict::Pass);
    assert_eq!(spf.domain, "example.com");
    assert!(provider_spf(message("hi").as_bytes(), SpfHeader::ReceivedSpf).is_none());
}
