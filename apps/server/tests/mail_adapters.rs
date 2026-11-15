//! Delivery adapters (spec §5.5): SMTP against an in-process server built on `run_session`,
//! direct MX delivery rules, and the HTTP adapters against a local fake API.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{NaiveDateTime, TimeZone, Utc};
use orbit_mail::{
    AcceptedRecipient, DataDecision, Envelope, NoTls, RcptDecision, Reply, SessionContext,
    SessionHandler, SessionLimits, run_session,
};
use orbit_platform::{Id, SigV4Credentials, SigV4Request, TimestampMillis, sigv4_authorization};
use orbit_server::mail_dns::FakeDns;
use orbit_server::mail_send::{
    DeliveryContext, Outgoing, RecipientOutcome, adapter_for, api_base, provider_headers,
    with_provider_headers,
};
use orbit_server::repositories::mail::{
    RouteSecrets, RouteSettings, SendRoute, SendRouteKind, SmtpSecurity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

const RAW: &[u8] = b"From: Alice <alice@example.com>\r\nTo: Bob <bob@x.test>\r\nSubject: Hi\r\n\r\nHello\r\n.leading dot\r\n";

// ---------------------------------------------------------------- helpers

fn route(kind: SendRouteKind, settings: RouteSettings) -> SendRoute {
    SendRoute {
        id: Id::new_v7(),
        workspace_id: None,
        kind,
        name: "Test".to_owned(),
        provider_id: None,
        settings,
        secrets_set: Vec::new(),
        last_sent_at: None,
        last_error: None,
        domain_count: 0,
        created_at: TimestampMillis::now(),
    }
}

fn context(dns: &FakeDns, allow_private: bool) -> DeliveryContext {
    DeliveryContext::new(
        Some("out.orbit.test".to_owned()),
        Arc::new(dns.clone()),
        allow_private,
    )
}

fn recipients(list: &[&str]) -> Vec<String> {
    list.iter().map(|address| (*address).to_owned()).collect()
}

async fn deliver(
    route: &SendRoute,
    secrets: &RouteSecrets,
    ctx: &DeliveryContext,
    mail_from: &str,
    to: &[String],
) -> HashMap<String, RecipientOutcome> {
    deliver_from(route, secrets, ctx, mail_from, "example.com", to).await
}

async fn deliver_from(
    route: &SendRoute,
    secrets: &RouteSecrets,
    ctx: &DeliveryContext,
    mail_from: &str,
    from_domain: &str,
    to: &[String],
) -> HashMap<String, RecipientOutcome> {
    let adapter = adapter_for(route, secrets, ctx);
    let outgoing = Outgoing {
        raw: RAW,
        mail_from,
        from_domain,
        recipients: to,
        correlation: "corr-1",
    };
    let outcomes = tokio::time::timeout(Duration::from_secs(30), adapter.deliver(&outgoing))
        .await
        .expect("delivery finishes");
    assert_eq!(outcomes.len(), to.len(), "one outcome for each recipient");
    outcomes.into_iter().collect()
}

fn accepted(outcome: &RecipientOutcome) -> bool {
    matches!(outcome, RecipientOutcome::Accepted { .. })
}

fn deferred_with(outcome: &RecipientOutcome, text: &str) -> bool {
    matches!(outcome, RecipientOutcome::Deferred { response } if response.contains(text))
}

fn bounced_with(outcome: &RecipientOutcome, text: &str) -> bool {
    matches!(outcome, RecipientOutcome::Bounced { response } if response.contains(text))
}

// ---------------------------------------------------------------- SMTP server

#[derive(Default)]
struct Scripted {
    refuse: HashMap<String, Reply>,
    connections: AtomicUsize,
    messages: Mutex<Vec<(Envelope, Vec<u8>)>>,
}

impl SessionHandler for Scripted {
    async fn rcpt(&self, _ctx: &SessionContext, rcpt: &str) -> RcptDecision {
        match self.refuse.get(rcpt) {
            Some(reply) => RcptDecision::Reject(*reply),
            None => RcptDecision::Accept(AcceptedRecipient {
                address_id: rcpt.to_owned(),
                workspace_id: "w".to_owned(),
                rcpt: rcpt.to_owned(),
            }),
        }
    }

    async fn data(&self, _ctx: &SessionContext, envelope: &Envelope, raw: Vec<u8>) -> DataDecision {
        self.messages.lock().unwrap().push((envelope.clone(), raw));
        DataDecision::Accepted
    }
}

/// A plain SMTP server (no STARTTLS) on 127.0.0.1; returns its port.
async fn start_smtp(handler: Arc<Scripted>) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                return;
            };
            handler.connections.fetch_add(1, Ordering::SeqCst);
            let handler = handler.clone();
            tokio::spawn(async move {
                let limits = SessionLimits {
                    greeting_delay: Duration::ZERO,
                    ..SessionLimits::default()
                };
                let ctx = SessionContext {
                    peer_ip: peer.ip(),
                    helo: None,
                    tls: None,
                };
                let _ = run_session(
                    stream,
                    ctx,
                    &*handler,
                    &limits,
                    "relay.test",
                    None::<&NoTls>,
                    &CancellationToken::new(),
                )
                .await;
            });
        }
    });
    port
}

fn smtp_route(host: &str, port: u16, security: SmtpSecurity) -> SendRoute {
    route(
        SendRouteKind::Smtp,
        RouteSettings {
            host: Some(host.to_owned()),
            port: Some(port),
            security,
            max_message_bytes: 25_000_000,
            ..RouteSettings::default()
        },
    )
}

const REFUSED: Reply = Reply {
    code: 550,
    enhanced: "5.1.1",
    text: "No such user",
};
const LATER: Reply = Reply {
    code: 451,
    enhanced: "4.2.0",
    text: "Mailbox busy, try later",
};

#[tokio::test]
async fn smtp_all_accepted() {
    let handler = Arc::new(Scripted::default());
    let port = start_smtp(handler.clone()).await;
    let to = recipients(&["bob@x.test", "carol@y.test"]);
    let outcomes = deliver(
        &smtp_route("127.0.0.1", port, SmtpSecurity::None),
        &RouteSecrets::None,
        &context(&FakeDns::new(), true),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(outcomes.values().all(accepted), "{outcomes:?}");
    let messages = handler.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    let (envelope, raw) = &messages[0];
    assert_eq!(envelope.mail_from.as_deref(), Some("bounce+t1@example.com"));
    let rcpts: Vec<&str> = envelope
        .recipients
        .iter()
        .map(|r| r.rcpt.as_str())
        .collect();
    assert_eq!(rcpts, ["bob@x.test", "carol@y.test"]);
    assert_eq!(raw.as_slice(), RAW, "dot-stuffing round trips");
}

#[tokio::test]
async fn smtp_one_rcpt_550_others_delivered() {
    let handler = Arc::new(Scripted {
        refuse: HashMap::from([("nobody@x.test".to_owned(), REFUSED)]),
        ..Scripted::default()
    });
    let port = start_smtp(handler.clone()).await;
    let to = recipients(&["bob@x.test", "nobody@x.test", "carol@x.test"]);
    let outcomes = deliver(
        &smtp_route("127.0.0.1", port, SmtpSecurity::None),
        &RouteSecrets::None,
        &context(&FakeDns::new(), true),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(accepted(&outcomes["bob@x.test"]));
    assert!(accepted(&outcomes["carol@x.test"]));
    assert!(
        bounced_with(&outcomes["nobody@x.test"], "550"),
        "{outcomes:?}"
    );
    let messages = handler.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    let rcpts: Vec<&str> = messages[0]
        .0
        .recipients
        .iter()
        .map(|r| r.rcpt.as_str())
        .collect();
    assert_eq!(rcpts, ["bob@x.test", "carol@x.test"]);
}

#[tokio::test]
async fn smtp_one_rcpt_451_deferred() {
    let handler = Arc::new(Scripted {
        refuse: HashMap::from([("busy@x.test".to_owned(), LATER)]),
        ..Scripted::default()
    });
    let port = start_smtp(handler.clone()).await;
    let to = recipients(&["busy@x.test", "bob@x.test"]);
    let outcomes = deliver(
        &smtp_route("127.0.0.1", port, SmtpSecurity::None),
        &RouteSecrets::None,
        &context(&FakeDns::new(), true),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(
        deferred_with(&outcomes["busy@x.test"], "451"),
        "{outcomes:?}"
    );
    assert!(accepted(&outcomes["bob@x.test"]));
    assert_eq!(handler.messages.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn smtp_every_rcpt_refused_sends_no_data() {
    let handler = Arc::new(Scripted {
        refuse: HashMap::from([("nobody@x.test".to_owned(), REFUSED)]),
        ..Scripted::default()
    });
    let port = start_smtp(handler.clone()).await;
    let to = recipients(&["nobody@x.test"]);
    let outcomes = deliver(
        &smtp_route("127.0.0.1", port, SmtpSecurity::None),
        &RouteSecrets::None,
        &context(&FakeDns::new(), true),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(bounced_with(&outcomes["nobody@x.test"], "5.1.1"));
    assert!(handler.messages.lock().unwrap().is_empty());
}

#[tokio::test]
async fn smtp_requires_tls_when_set() {
    let handler = Arc::new(Scripted::default());
    let port = start_smtp(handler.clone()).await;
    let to = recipients(&["bob@x.test"]);
    for security in [SmtpSecurity::Starttls] {
        let outcomes = deliver(
            &smtp_route("127.0.0.1", port, security),
            &RouteSecrets::Smtp {
                username: "user".to_owned(),
                password: "secret".to_owned(),
            },
            &context(&FakeDns::new(), true),
            "bounce+t1@example.com",
            &to,
        )
        .await;
        assert!(
            deferred_with(&outcomes["bob@x.test"], "TLS required"),
            "{outcomes:?}"
        );
    }
    assert!(handler.messages.lock().unwrap().is_empty());
}

#[tokio::test]
async fn smtp_tls_wrapper_never_falls_back_to_plain_text() {
    let handler = Arc::new(Scripted::default());
    let port = start_smtp(handler.clone()).await;
    let to = recipients(&["bob@x.test"]);
    let outcomes = deliver(
        &smtp_route("127.0.0.1", port, SmtpSecurity::Tls),
        &RouteSecrets::Smtp {
            username: "user".to_owned(),
            password: "secret".to_owned(),
        },
        &context(&FakeDns::new(), true),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(
        matches!(outcomes["bob@x.test"], RecipientOutcome::Deferred { .. }),
        "{outcomes:?}"
    );
    assert!(handler.messages.lock().unwrap().is_empty());
}

/// A server that answers the greeting and EHLO (no STARTTLS), then never answers anything
/// (QUIT included), and keeps the connection open.
async fn start_silent_smtp() -> u16 {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                let _ = write.write_all(b"220 silent.test ESMTP\r\n").await;
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.to_ascii_uppercase().starts_with("EHLO") {
                        let _ = write
                            .write_all(b"250-silent.test\r\n250 8BITMIME\r\n")
                            .await;
                    }
                }
            });
        }
    });
    port
}

#[tokio::test]
async fn smtp_gives_up_on_a_server_that_never_answers_quit() {
    let port = start_silent_smtp().await;
    let to = recipients(&["bob@x.test"]);
    let started = std::time::Instant::now();
    let outcomes = deliver(
        &smtp_route("127.0.0.1", port, SmtpSecurity::Starttls),
        &RouteSecrets::None,
        &context(&FakeDns::new(), true),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(
        deferred_with(&outcomes["bob@x.test"], "TLS required"),
        "{outcomes:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn smtp_plain_route_refused_on_public_address() {
    // A plain relay is only allowed to a private address, even for the root user.
    let dns = FakeDns::new();
    dns.set_a("relay.example.com", &[Ipv4Addr::new(93, 184, 216, 34)]);
    let to = recipients(&["bob@x.test"]);
    let outcomes = deliver(
        &smtp_route("relay.example.com", 25, SmtpSecurity::None),
        &RouteSecrets::None,
        &context(&dns, true),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(
        deferred_with(&outcomes["bob@x.test"], "refused"),
        "{outcomes:?}"
    );
}

/// DNS rebinding: the host was public when the route was saved; now it resolves to a private
/// address. A workspace route must not connect.
#[tokio::test]
async fn workspace_route_refuses_private_address_at_send_time() {
    let handler = Arc::new(Scripted::default());
    let port = start_smtp(handler.clone()).await;
    let dns = FakeDns::new();
    dns.set_a("relay.example.com", &[Ipv4Addr::LOCALHOST]);
    let to = recipients(&["bob@x.test"]);
    for host in ["relay.example.com", "127.0.0.1", "[::1]"] {
        for security in [
            SmtpSecurity::Starttls,
            SmtpSecurity::Tls,
            SmtpSecurity::None,
        ] {
            let outcomes = deliver(
                &smtp_route(host, port, security),
                &RouteSecrets::None,
                &context(&dns, false),
                "bounce+t1@example.com",
                &to,
            )
            .await;
            assert!(
                deferred_with(&outcomes["bob@x.test"], "refused"),
                "{host} {security:?}: {outcomes:?}"
            );
        }
    }
    // A mix of a public and a private address is refused too.
    dns.set_a(
        "relay.example.com",
        &[Ipv4Addr::new(93, 184, 216, 34), Ipv4Addr::new(10, 0, 0, 1)],
    );
    let outcomes = deliver(
        &smtp_route("relay.example.com", port, SmtpSecurity::Starttls),
        &RouteSecrets::None,
        &context(&dns, false),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(deferred_with(&outcomes["bob@x.test"], "refused"));
    assert_eq!(
        handler.connections.load(Ordering::SeqCst),
        0,
        "never connected"
    );
}

#[tokio::test]
async fn smtp_unresolvable_host_is_deferred() {
    let to = recipients(&["bob@x.test"]);
    let outcomes = deliver(
        &smtp_route("gone.example.com", 587, SmtpSecurity::Starttls),
        &RouteSecrets::None,
        &context(&FakeDns::new(), false),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(matches!(
        outcomes["bob@x.test"],
        RecipientOutcome::Deferred { .. }
    ));
}

#[tokio::test]
async fn message_over_route_limit_bounces() {
    let mut route = smtp_route("127.0.0.1", 1, SmtpSecurity::None);
    route.settings.max_message_bytes = 10;
    let to = recipients(&["bob@x.test"]);
    let outcomes = deliver(
        &route,
        &RouteSecrets::None,
        &context(&FakeDns::new(), true),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(
        bounced_with(&outcomes["bob@x.test"], "5.3.4"),
        "{outcomes:?}"
    );
}

// ---------------------------------------------------------------- direct

fn direct_route() -> SendRoute {
    route(
        SendRouteKind::Direct,
        RouteSettings {
            max_message_bytes: 25_000_000,
            ..RouteSettings::default()
        },
    )
}

#[tokio::test]
async fn direct_null_mx_bounces() {
    let dns = FakeDns::new();
    dns.set_mx("null.example", &[(0, ".")]);
    let to = recipients(&["a@null.example", "b@null.example"]);
    let outcomes = deliver(
        &direct_route(),
        &RouteSecrets::None,
        &context(&dns, false),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    for address in &to {
        assert!(bounced_with(&outcomes[address], "5.1.10"), "{outcomes:?}");
    }
}

#[tokio::test]
async fn direct_refuses_private_mx() {
    let dns = FakeDns::new();
    dns.set_mx("private.example", &[(10, "mx.private.example")]);
    dns.set_a("mx.private.example", &[Ipv4Addr::new(10, 1, 2, 3)]);
    dns.set_mx("loop.example", &[(10, "mx.loop.example")]);
    dns.set_a("mx.loop.example", &[Ipv4Addr::LOCALHOST]);
    dns.set_aaaa("mx.loop.example", &["fe80::1".parse().unwrap()]);
    // No MX: the A record of the domain is the mail server (and it is the cloud metadata address).
    dns.set_a("meta.example", &[Ipv4Addr::new(169, 254, 169, 254)]);
    // The server's own address.
    dns.set_a("out.orbit.test", &[Ipv4Addr::new(93, 184, 216, 34)]);
    dns.set_mx("self.example", &[(10, "mx.self.example")]);
    dns.set_a("mx.self.example", &[Ipv4Addr::new(93, 184, 216, 34)]);
    let to = recipients(&[
        "a@private.example",
        "b@loop.example",
        "c@meta.example",
        "d@self.example",
    ]);
    let outcomes = deliver(
        &direct_route(),
        &RouteSecrets::None,
        &context(&dns, false),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    for address in &to {
        assert!(bounced_with(&outcomes[address], "5.4.4"), "{outcomes:?}");
    }
    // MX hosts come from remote DNS: an instance route (allow_private) is refused the same.
    let outcomes = deliver(
        &direct_route(),
        &RouteSecrets::None,
        &context(&dns, true),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    for address in &to {
        assert!(bounced_with(&outcomes[address], "5.4.4"), "{outcomes:?}");
    }
}

#[tokio::test]
async fn direct_domain_without_address_bounces_and_dns_failure_defers() {
    let dns = FakeDns::new();
    dns.fail("flaky.example", orbit_server::mail_dns::DnsError::Timeout);
    let to = recipients(&["a@nowhere.example", "b@flaky.example"]);
    let outcomes = deliver(
        &direct_route(),
        &RouteSecrets::None,
        &context(&dns, false),
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(
        matches!(
            outcomes["a@nowhere.example"],
            RecipientOutcome::Bounced { .. }
        ),
        "{outcomes:?}"
    );
    assert!(matches!(
        outcomes["b@flaky.example"],
        RecipientOutcome::Deferred { .. }
    ));
}

#[tokio::test]
async fn direct_without_hostname_defers() {
    let mut ctx = context(&FakeDns::new(), false);
    ctx.hostname = None;
    let to = recipients(&["a@x.test"]);
    let outcomes = deliver(
        &direct_route(),
        &RouteSecrets::None,
        &ctx,
        "bounce+t1@example.com",
        &to,
    )
    .await;
    assert!(deferred_with(&outcomes["a@x.test"], "ORBIT_MAIL_HOSTNAME"));
}

// ---------------------------------------------------------------- HTTP fakes

#[derive(Clone, Debug)]
struct Captured {
    method: String,
    path: String,
    headers: HeaderMap,
    body: Bytes,
}

#[derive(Clone)]
struct Api {
    requests: Arc<Mutex<Vec<Captured>>>,
    status: StatusCode,
    body: Arc<dyn Fn(&Captured) -> String + Send + Sync>,
}

async fn record(State(api): State<Api>, request: Request<Body>) -> impl IntoResponse {
    let (parts, body) = request.into_parts();
    let captured = Captured {
        method: parts.method.to_string(),
        path: parts.uri.path().to_owned(),
        headers: parts.headers,
        body: to_bytes(body, usize::MAX).await.unwrap(),
    };
    let reply = (api.body)(&captured);
    api.requests.lock().unwrap().push(captured);
    (api.status, reply)
}

async fn start_api(
    status: StatusCode,
    body: impl Fn(&Captured) -> String + Send + Sync + 'static,
) -> (String, Arc<Mutex<Vec<Captured>>>) {
    let api = Api {
        requests: Arc::new(Mutex::new(Vec::new())),
        status,
        body: Arc::new(body),
    };
    let requests = api.requests.clone();
    let app = axum::Router::new().fallback(record).with_state(api);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, requests)
}

fn mailgun_route(base: &str) -> SendRoute {
    route(
        SendRouteKind::Mailgun,
        RouteSettings {
            region: Some("us".to_owned()),
            base_url: Some(base.to_owned()),
            max_message_bytes: 25_000_000,
            ..RouteSettings::default()
        },
    )
}

fn mailgun_secrets() -> RouteSecrets {
    RouteSecrets::Mailgun {
        api_key: "key-1".to_owned(),
        webhook_signing_key: "sign".to_owned(),
    }
}

#[tokio::test]
async fn mailgun_mime_request_shape() {
    let (base, requests) = start_api(StatusCode::OK, |_| {
        json!({ "id": "<mg-1@example.com>", "message": "Queued. Thank you." }).to_string()
    })
    .await;
    let to = recipients(&["bob@x.test", "carol@y.test"]);
    let outcomes = deliver(
        &mailgun_route(&base),
        &mailgun_secrets(),
        &context(&FakeDns::new(), true),
        "alice@example.com",
        &to,
    )
    .await;
    for address in &to {
        assert_eq!(
            outcomes[address],
            RecipientOutcome::Accepted {
                provider_message_id: Some("<mg-1@example.com>".to_owned())
            }
        );
    }
    let requests = requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v3/example.com/messages.mime");
    assert_eq!(
        request.headers["authorization"].to_str().unwrap(),
        format!("Basic {}", BASE64.encode("api:key-1"))
    );
    let content_type = request.headers["content-type"].to_str().unwrap();
    assert!(content_type.starts_with("multipart/form-data"));
    let boundary = multer::parse_boundary(content_type).unwrap();
    let body = request.body.clone();
    let mut multipart = multer::Multipart::new(
        futures_util::stream::once(async move { Ok::<_, std::io::Error>(body) }),
        boundary,
    );
    let mut fields: Vec<(String, Vec<u8>)> = Vec::new();
    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap().to_owned();
        fields.push((name, field.bytes().await.unwrap().to_vec()));
    }
    assert_eq!(
        fields,
        vec![
            ("to".to_owned(), b"bob@x.test".to_vec()),
            ("to".to_owned(), b"carol@y.test".to_vec()),
            ("message".to_owned(), RAW.to_vec()),
        ]
    );
}

#[tokio::test]
async fn http_429_is_deferred() {
    let (base, _) = start_api(StatusCode::TOO_MANY_REQUESTS, |_| "slow down".to_owned()).await;
    let to = recipients(&["bob@x.test", "carol@y.test"]);
    let outcomes = deliver(
        &mailgun_route(&base),
        &mailgun_secrets(),
        &context(&FakeDns::new(), true),
        "alice@example.com",
        &to,
    )
    .await;
    for address in &to {
        assert!(deferred_with(&outcomes[address], "429"), "{outcomes:?}");
    }

    let (base, _) = start_api(StatusCode::BAD_GATEWAY, |_| String::new()).await;
    let outcomes = deliver(
        &mailgun_route(&base),
        &mailgun_secrets(),
        &context(&FakeDns::new(), true),
        "alice@example.com",
        &to,
    )
    .await;
    assert!(outcomes.values().all(|o| deferred_with(o, "502")));

    // 4xx: bounced, with the body as the response.
    let (base, _) = start_api(StatusCode::BAD_REQUEST, |_| {
        "{\"message\":\"bad to\"}".to_owned()
    })
    .await;
    let outcomes = deliver(
        &mailgun_route(&base),
        &mailgun_secrets(),
        &context(&FakeDns::new(), true),
        "alice@example.com",
        &to,
    )
    .await;
    assert!(
        outcomes.values().all(|o| bounced_with(o, "bad to")),
        "{outcomes:?}"
    );

    // Nothing listening: deferred.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let outcomes = deliver(
        &mailgun_route(&closed),
        &mailgun_secrets(),
        &context(&FakeDns::new(), true),
        "alice@example.com",
        &to,
    )
    .await;
    assert!(
        outcomes
            .values()
            .all(|o| matches!(o, RecipientOutcome::Deferred { .. }))
    );
}

#[tokio::test]
async fn ses_raw_request_signed_and_batched_by_50() {
    let (base, requests) = start_api(StatusCode::OK, |captured| {
        let calls = captured.body.len();
        json!({ "MessageId": format!("ses-{calls}") }).to_string()
    })
    .await;
    let to: Vec<String> = (0..120).map(|n| format!("user{n}@x.test")).collect();
    let ses = route(
        SendRouteKind::Ses,
        RouteSettings {
            region: Some("us-east-1".to_owned()),
            base_url: Some(base.clone()),
            max_message_bytes: 40_000_000,
            ..RouteSettings::default()
        },
    );
    let secrets = RouteSecrets::Ses {
        access_key_id: "AKIDEXAMPLE".to_owned(),
        secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned(),
    };
    let outcomes = deliver(
        &ses,
        &secrets,
        &context(&FakeDns::new(), true),
        "alice@example.com",
        &to,
    )
    .await;
    assert!(outcomes.values().all(accepted), "{outcomes:?}");
    let requests = requests.lock().unwrap();
    let sizes: Vec<usize> = requests
        .iter()
        .map(|request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            body["Destination"]["ToAddresses"].as_array().unwrap().len()
        })
        .collect();
    assert_eq!(sizes, [50, 50, 20]);
    let mut seen: Vec<String> = Vec::new();
    for request in requests.iter() {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v2/email/outbound-emails");
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let data = body["Content"]["Raw"]["Data"].as_str().unwrap();
        assert_eq!(BASE64.decode(data).unwrap(), RAW);
        for address in body["Destination"]["ToAddresses"].as_array().unwrap() {
            seen.push(address.as_str().unwrap().to_owned());
        }
        let authorization = request.headers["authorization"].to_str().unwrap();
        assert!(authorization.starts_with("AWS4-HMAC-SHA256 "));
        assert!(
            authorization.contains("Credential=AKIDEXAMPLE/")
                && authorization.contains("/us-east-1/ses/aws4_request"),
            "{authorization}"
        );
        // Sign the received request again: the signature covers what was sent.
        let amz_date = request.headers["x-amz-date"].to_str().unwrap();
        let now = Utc
            .from_utc_datetime(&NaiveDateTime::parse_from_str(amz_date, "%Y%m%dT%H%M%SZ").unwrap());
        let signed = authorization
            .split("SignedHeaders=")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
            .unwrap();
        let mut headers: Vec<(String, String)> = signed
            .split(';')
            .filter(|name| !matches!(*name, "x-amz-date" | "x-amz-content-sha256"))
            .map(|name| {
                (
                    name.to_owned(),
                    request.headers[name].to_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert!(headers.iter().any(|(name, _)| name == "host"));
        let payload_hash = format!("{:x}", Sha256::digest(&request.body));
        assert_eq!(
            request.headers["x-amz-content-sha256"].to_str().unwrap(),
            payload_hash
        );
        let expected = sigv4_authorization(
            "ses",
            "us-east-1",
            &SigV4Credentials {
                access_key_id: "AKIDEXAMPLE",
                secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            },
            &SigV4Request {
                method: "POST",
                path: "/v2/email/outbound-emails",
                query: "",
                payload_hash: &payload_hash,
                now,
            },
            &mut headers,
        );
        assert_eq!(authorization, expected);
    }
    assert_eq!(seen, to, "every recipient once, in order");
}

#[tokio::test]
async fn cloudflare_send_raw_shape() {
    let (base, requests) = start_api(StatusCode::OK, |_| {
        json!({
            "success": true,
            "errors": [],
            "messages": [],
            "result": {
                "delivered": ["bob@x.test"],
                "queued": ["carol@y.test"],
                "permanent_bounces": ["gone@z.test"],
                "suppressed_recipients": [],
                "message_id": "<cf-1@example.com>"
            }
        })
        .to_string()
    })
    .await;
    let cloudflare = route(
        SendRouteKind::Cloudflare,
        RouteSettings {
            account_id: Some("0123456789abcdef0123456789abcdef".to_owned()),
            base_url: Some(base),
            max_message_bytes: 26_214_400,
            ..RouteSettings::default()
        },
    );
    let secrets = RouteSecrets::Cloudflare {
        api_token: "cf-token".to_owned(),
        event_secret: "events".to_owned(),
    };
    let to = recipients(&["bob@x.test", "carol@y.test", "gone@z.test"]);
    let outcomes = deliver(
        &cloudflare,
        &secrets,
        &context(&FakeDns::new(), true),
        "alice@example.com",
        &to,
    )
    .await;
    let id = Some("<cf-1@example.com>".to_owned());
    assert_eq!(
        outcomes["bob@x.test"],
        RecipientOutcome::Accepted {
            provider_message_id: id.clone()
        }
    );
    assert_eq!(
        outcomes["carol@y.test"],
        RecipientOutcome::Accepted {
            provider_message_id: id
        }
    );
    assert!(matches!(
        outcomes["gone@z.test"],
        RecipientOutcome::Bounced { .. }
    ));
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.path,
        "/accounts/0123456789abcdef0123456789abcdef/email/sending/send_raw"
    );
    assert_eq!(
        request.headers["authorization"].to_str().unwrap(),
        "Bearer cf-token"
    );
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        body,
        json!({
            "from": "alice@example.com",
            "recipients": ["bob@x.test", "carol@y.test", "gone@z.test"],
            "mime_message": std::str::from_utf8(RAW).unwrap(),
        })
    );
}

#[test]
fn base_url_only_for_instance_routes() {
    // A workspace route never has a base URL (the repository clears it); if one is there, the
    // adapter still calls the provider's own host.
    let mut settings = RouteSettings {
        region: Some("eu".to_owned()),
        base_url: Some("http://10.0.0.5/".to_owned()),
        ..RouteSettings::default()
    };
    assert_eq!(
        api_base(SendRouteKind::Mailgun, &settings, false).unwrap(),
        "https://api.eu.mailgun.net"
    );
    assert_eq!(
        api_base(SendRouteKind::Mailgun, &settings, true).unwrap(),
        "http://10.0.0.5"
    );
    settings.region = Some("au".to_owned());
    assert_eq!(
        api_base(SendRouteKind::Mailgun, &settings, false).unwrap(),
        "https://api.au.mailgun.net"
    );
    settings.region = Some("eu-west-1".to_owned());
    assert_eq!(
        api_base(SendRouteKind::Ses, &settings, false).unwrap(),
        "https://email.eu-west-1.amazonaws.com"
    );
    settings.region = Some("evil.example/x".to_owned());
    assert!(api_base(SendRouteKind::Ses, &settings, false).is_err());
    assert_eq!(
        api_base(SendRouteKind::Cloudflare, &settings, false).unwrap(),
        "https://api.cloudflare.com/client/v4"
    );
}

#[tokio::test]
async fn mailgun_route_errors_are_deferred() {
    let to = recipients(&["bob@x.test", "carol@y.test"]);
    for (status, body) in [
        (StatusCode::UNAUTHORIZED, "Forbidden"),
        (
            StatusCode::NOT_FOUND,
            "{\"message\":\"Domain not found: example.com\"}",
        ),
    ] {
        let (base, _) = start_api(status, move |_| body.to_owned()).await;
        let outcomes = deliver(
            &mailgun_route(&base),
            &mailgun_secrets(),
            &context(&FakeDns::new(), true),
            "alice@example.com",
            &to,
        )
        .await;
        for address in &to {
            assert!(
                deferred_with(&outcomes[address], "Route error"),
                "{status}: {outcomes:?}"
            );
        }
    }
}

#[tokio::test]
async fn ses_route_errors_are_deferred() {
    let (base, _) = start_api(StatusCode::BAD_REQUEST, |_| {
        json!({ "message": "Email address is not verified. The following identities failed the check in region US-EAST-1: alice@example.com" })
            .to_string()
    })
    .await;
    let ses = route(
        SendRouteKind::Ses,
        RouteSettings {
            region: Some("us-east-1".to_owned()),
            base_url: Some(base),
            max_message_bytes: 40_000_000,
            ..RouteSettings::default()
        },
    );
    let secrets = RouteSecrets::Ses {
        access_key_id: "AKIDEXAMPLE".to_owned(),
        secret_access_key: "secret".to_owned(),
    };
    let to = recipients(&["bob@x.test"]);
    let outcomes = deliver(
        &ses,
        &secrets,
        &context(&FakeDns::new(), true),
        "alice@example.com",
        &to,
    )
    .await;
    assert!(
        deferred_with(&outcomes["bob@x.test"], "Route error"),
        "{outcomes:?}"
    );
}

#[tokio::test]
async fn mailgun_sends_for_the_from_domain_as_a_label() {
    let (base, requests) =
        start_api(StatusCode::OK, |_| json!({ "id": "<mg-2@x>" }).to_string()).await;
    let to = recipients(&["bob@x.test"]);
    let outcomes = deliver_from(
        &mailgun_route(&base),
        &mailgun_secrets(),
        &context(&FakeDns::new(), true),
        // The envelope sender may be a provider's or a bounce address: not the API domain.
        "bounces@mg.provider.test",
        "Bücher.example",
        &to,
    )
    .await;
    assert!(accepted(&outcomes["bob@x.test"]), "{outcomes:?}");
    let requests = requests.lock().unwrap().clone();
    assert_eq!(requests[0].path, "/v3/xn--bcher-kva.example/messages.mime");
}

// ---------------------------------------------------------------- headers

#[test]
fn provider_headers_per_kind() {
    let settings = RouteSettings {
        stream: Some("outbound".to_owned()),
        ..RouteSettings::default()
    };
    let pairs = |kind| {
        provider_headers(kind, &settings, "corr-1")
            .into_iter()
            .collect::<Vec<(String, String)>>()
    };
    let owned = |list: &[(&str, &str)]| {
        list.iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        pairs(SendRouteKind::Postmark),
        owned(&[
            ("X-PM-KeepID", "true"),
            ("X-PM-TrackLinks", "None"),
            ("X-PM-Message-Stream", "outbound"),
            ("X-PM-Metadata-orbit", "corr-1"),
        ])
    );
    assert_eq!(
        pairs(SendRouteKind::Mailgun),
        owned(&[
            ("X-Mailgun-Track", "no"),
            ("X-Mailgun-Variables", "{\"orbit\":\"corr-1\"}"),
        ])
    );
    assert_eq!(
        pairs(SendRouteKind::Resend),
        owned(&[("Resend-Idempotency-Key", "corr-1")])
    );
    assert_eq!(
        pairs(SendRouteKind::Postshiba),
        owned(&[("X-SMTPAPI", "{\"unique_args\":{\"orbit\":\"corr-1\"}}")])
    );
    for kind in [
        SendRouteKind::Direct,
        SendRouteKind::Smtp,
        SendRouteKind::Ses,
        SendRouteKind::Cloudflare,
    ] {
        assert!(pairs(kind).is_empty(), "{kind:?}");
    }
    // Postmark's default stream; a correlation with a line break cannot add a header.
    let headers = provider_headers(
        SendRouteKind::Postmark,
        &RouteSettings::default(),
        "a\r\nBcc: x@evil.test",
    );
    assert!(headers.contains(&("X-PM-Message-Stream".to_owned(), "outbound".to_owned())));
    assert!(
        headers
            .iter()
            .all(|(_, value)| !value.contains('\r') && !value.contains('\n'))
    );

    let prefixed = with_provider_headers(&pairs(SendRouteKind::Resend), RAW);
    assert_eq!(
        prefixed,
        [b"Resend-Idempotency-Key: corr-1\r\n".as_slice(), RAW].concat()
    );
}
