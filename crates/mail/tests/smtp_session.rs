use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbit_mail::{
    AcceptedRecipient, DataDecision, Envelope, NoTls, RcptDecision, Reply, SessionContext,
    SessionHandler, SessionLimits, StartTls, TlsInfo, run_session,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, DuplexStream};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const HOST: &str = "mx.orbit.test";

#[derive(Default)]
struct FakeHandler {
    rcpts: Mutex<Vec<String>>,
    messages: Mutex<Vec<(Envelope, Vec<u8>, SessionContext)>>,
}

impl SessionHandler for FakeHandler {
    async fn rcpt(&self, _ctx: &SessionContext, rcpt: &str) -> RcptDecision {
        self.rcpts.lock().unwrap().push(rcpt.to_owned());
        match rcpt.split_once('@') {
            Some(("nobody", "orbit.test")) => RcptDecision::Reject(Reply::USER_UNKNOWN),
            Some((_, "orbit.test")) => RcptDecision::Accept(AcceptedRecipient {
                address_id: format!("addr-{rcpt}"),
                workspace_id: "ws-1".into(),
                rcpt: rcpt.to_owned(),
            }),
            _ => RcptDecision::Reject(Reply::RELAY_DENIED),
        }
    }

    async fn data(&self, ctx: &SessionContext, envelope: &Envelope, raw: Vec<u8>) -> DataDecision {
        self.messages
            .lock()
            .unwrap()
            .push((envelope.clone(), raw, ctx.clone()));
        DataDecision::Accepted
    }
}

/// A "TLS" upgrade that keeps the plain stream, so the STARTTLS state rules can be tested without rustls.
struct FakeTls;

impl StartTls for FakeTls {
    type Stream<S>
        = S
    where
        S: AsyncRead + AsyncWrite + Unpin + Send;

    async fn accept<S>(&self, stream: S) -> io::Result<(S, TlsInfo)>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        Ok((
            stream,
            TlsInfo {
                version: "TLSv1.3".into(),
                cipher: "TEST".into(),
            },
        ))
    }
}

struct Client {
    io: BufReader<DuplexStream>,
    handler: Arc<FakeHandler>,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

fn limits() -> SessionLimits {
    SessionLimits::default()
}

fn context() -> SessionContext {
    SessionContext {
        peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
        helo: None,
        tls: None,
    }
}

fn start(limits: SessionLimits, tls: bool) -> Client {
    let (client, server) = tokio::io::duplex(1 << 20);
    let handler = Arc::new(FakeHandler::default());
    let cancel = CancellationToken::new();
    let task = {
        let handler = handler.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let result = if tls {
                run_session(
                    server,
                    context(),
                    &*handler,
                    &limits,
                    HOST,
                    Some(&FakeTls),
                    &cancel,
                )
                .await
            } else {
                run_session(
                    server,
                    context(),
                    &*handler,
                    &limits,
                    HOST,
                    None::<&NoTls>,
                    &cancel,
                )
                .await
            };
            result.unwrap();
        })
    };
    Client {
        io: BufReader::new(client),
        handler,
        cancel,
        task,
    }
}

impl Client {
    /// Reads one (possibly multi-line) reply and returns its lines without CRLF.
    async fn reply(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let n = self.io.read_line(&mut line).await.unwrap();
            assert!(
                n > 0,
                "connection closed while waiting for a reply; got {lines:?}"
            );
            let line = line.trim_end_matches("\r\n").to_owned();
            let last = line.as_bytes().get(3) != Some(&b'-');
            lines.push(line);
            if last {
                return lines;
            }
        }
    }

    /// Reads one reply and returns its last line.
    async fn line(&mut self) -> String {
        self.reply().await.pop().unwrap()
    }

    async fn send(&mut self, bytes: &[u8]) {
        self.io.get_mut().write_all(bytes).await.unwrap();
    }

    async fn cmd(&mut self, line: &str) -> String {
        self.send(format!("{line}\r\n").as_bytes()).await;
        self.line().await
    }

    async fn greet(&mut self) {
        assert!(self.line().await.starts_with("220 mx.orbit.test"));
    }

    async fn ehlo(&mut self) -> Vec<String> {
        self.send(b"EHLO client.example\r\n").await;
        self.reply().await
    }

    /// Asserts that the server closed the connection.
    async fn closed(mut self) -> Arc<FakeHandler> {
        let mut rest = String::new();
        let n = self.io.read_line(&mut rest).await.unwrap_or(0);
        assert_eq!(n, 0, "expected the connection to close, got {rest:?}");
        self.task.await.unwrap();
        self.handler
    }

    async fn quit(mut self) -> Arc<FakeHandler> {
        assert!(self.cmd("QUIT").await.starts_with("221 2.0.0"));
        self.closed().await
    }
}

fn code(line: &str) -> &str {
    &line[..9.min(line.len())]
}

#[tokio::test(start_paused = true)]
async fn happy_path() {
    let mut c = start(limits(), false);
    c.greet().await;
    let ehlo = c.ehlo().await;
    assert_eq!(ehlo[0], "250-mx.orbit.test");
    assert!(ehlo.contains(&"250-PIPELINING".to_owned()));
    assert!(ehlo.contains(&"250-8BITMIME".to_owned()));
    assert!(ehlo.contains(&"250-SIZE 52428800".to_owned()));
    assert_eq!(ehlo.last().unwrap(), "250 ENHANCEDSTATUSCODES");
    assert!(
        !ehlo
            .iter()
            .any(|l| l.contains("STARTTLS") || l.contains("AUTH"))
    );
    assert!(
        !ehlo
            .iter()
            .any(|l| l.contains("CHUNKING") || l.contains("SMTPUTF8"))
    );

    assert_eq!(
        code(&c.cmd("MAIL FROM:<ann@example.com> SIZE=100").await),
        "250 2.1.0"
    );
    assert_eq!(
        code(&c.cmd("RCPT TO:<support@orbit.test>").await),
        "250 2.1.5"
    );
    assert!(c.cmd("DATA").await.starts_with("354 "));
    c.send(b"Subject: hi\r\n\r\n..leading dot\r\nbody\r\n.\r\n")
        .await;
    assert_eq!(code(&c.line().await), "250 2.0.0");
    let handler = c.quit().await;

    let messages = handler.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    let (envelope, raw, ctx) = &messages[0];
    assert_eq!(raw, b"Subject: hi\r\n\r\n.leading dot\r\nbody\r\n");
    assert_eq!(envelope.mail_from.as_deref(), Some("ann@example.com"));
    assert_eq!(envelope.declared_size, Some(100));
    assert_eq!(envelope.recipients.len(), 1);
    assert_eq!(envelope.recipients[0].rcpt, "support@orbit.test");
    assert_eq!(ctx.helo.as_deref(), Some("client.example"));
}

#[tokio::test(start_paused = true)]
async fn pipelined_mail_rcpt_rcpt_data() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    c.send(
        b"MAIL FROM:<ann@example.com>\r\nRCPT TO:<a@orbit.test>\r\nRCPT TO:<nobody@orbit.test>\r\n\
          RCPT TO:<b@orbit.test>\r\nDATA\r\n",
    )
    .await;
    assert_eq!(code(&c.line().await), "250 2.1.0");
    assert_eq!(code(&c.line().await), "250 2.1.5");
    assert_eq!(code(&c.line().await), "550 5.1.1");
    assert_eq!(code(&c.line().await), "250 2.1.5");
    assert!(c.line().await.starts_with("354 "));
    c.send(b"hello\r\n.\r\n").await;
    assert_eq!(code(&c.line().await), "250 2.0.0");
    let handler = c.quit().await;
    let messages = handler.messages.lock().unwrap();
    let rcpts: Vec<_> = messages[0]
        .0
        .recipients
        .iter()
        .map(|r| r.rcpt.as_str())
        .collect();
    assert_eq!(rcpts, ["a@orbit.test", "b@orbit.test"]);
}

#[tokio::test(start_paused = true)]
async fn mail_before_ehlo_and_data_without_rcpt_are_refused() {
    let mut c = start(limits(), false);
    c.greet().await;
    assert_eq!(
        code(&c.cmd("MAIL FROM:<ann@example.com>").await),
        "503 5.5.1"
    );
    assert_eq!(code(&c.cmd("RCPT TO:<a@orbit.test>").await), "503 5.5.1");
    assert_eq!(c.cmd("HELO client.example").await, "250 mx.orbit.test");
    assert_eq!(code(&c.cmd("RCPT TO:<a@orbit.test>").await), "503 5.5.1");
    assert_eq!(
        code(&c.cmd("MAIL FROM:<ann@example.com>").await),
        "250 2.1.0"
    );
    assert_eq!(
        code(&c.cmd("MAIL FROM:<ann@example.com>").await),
        "503 5.5.1"
    );
    assert_eq!(code(&c.cmd("DATA").await), "503 5.5.1");
    assert_eq!(code(&c.cmd("RSET").await), "250 2.0.0");
    assert_eq!(code(&c.cmd("NOOP").await), "250 2.0.0");
    assert_eq!(
        code(&c.cmd("AUTH PLAIN AGFubgBzZWNyZXQ=").await),
        "502 5.5.1"
    );
    assert_eq!(code(&c.cmd("BDAT 10 LAST").await), "502 5.5.1");
    assert_eq!(code(&c.cmd("FOO bar").await), "500 5.5.1");
    assert_eq!(
        code(&c.cmd("MAIL FROM:<ann@example.com> SMTPUTF8").await),
        "555 5.5.4"
    );
    c.quit().await;
}

#[tokio::test(start_paused = true)]
async fn rcpt_passes_the_raw_path_to_the_handler() {
    // smtp-proto unquotes local parts and drops source routes; the handler must see what the client sent so that
    // `normalize_address` can refuse those forms.
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    c.cmd("MAIL FROM:<ann@example.com>").await;
    c.cmd("RCPT TO:<\"q\"@orbit.test>").await;
    c.cmd("RCPT TO:<@relay.example:a@orbit.test>").await;
    c.cmd("RCPT TO:<a%b@orbit.test>").await;
    let handler = c.quit().await;
    let rcpts = handler.rcpts.lock().unwrap();
    assert_eq!(
        *rcpts,
        [
            "\"q\"@orbit.test",
            "@relay.example:a@orbit.test",
            "a%b@orbit.test"
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn smuggling_bare_lf_in_data() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    c.cmd("MAIL FROM:<ann@example.com>").await;
    c.cmd("RCPT TO:<a@orbit.test>").await;
    c.cmd("DATA").await;
    c.send(b"Subject: x\r\n\r\nline one\nline two\r\n.\r\n")
        .await;
    assert_eq!(code(&c.line().await), "550 5.6.0");
    let handler = c.closed().await;
    assert!(handler.messages.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn smuggling_bare_cr_in_data() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    c.cmd("MAIL FROM:<ann@example.com>").await;
    c.cmd("RCPT TO:<a@orbit.test>").await;
    c.cmd("DATA").await;
    c.send(b"Subject: x\r\n\r\nbody\r.\r\nMAIL FROM:<eve@example.com>\r\n")
        .await;
    assert_eq!(code(&c.line().await), "550 5.6.0");
    let handler = c.closed().await;
    assert!(handler.messages.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn smuggling_lf_dot_lf_does_not_end_data() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    c.cmd("MAIL FROM:<ann@example.com>").await;
    c.cmd("RCPT TO:<a@orbit.test>").await;
    c.cmd("DATA").await;
    c.send(
        b"Subject: x\r\n\r\nbody\n.\nMAIL FROM:<admin@orbit.test>\r\nRCPT TO:<a@orbit.test>\r\nDATA\r\n\
          Subject: forged\r\n\r\n.\r\n",
    )
    .await;
    assert_eq!(code(&c.line().await), "550 5.6.0");
    let handler = c.closed().await;
    assert!(handler.messages.lock().unwrap().is_empty());
    assert_eq!(handler.rcpts.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn bare_lf_in_command() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.send(b"EHLO client.example\n").await;
    assert_eq!(code(&c.line().await), "550 5.6.0");
    c.closed().await;
}

#[tokio::test(start_paused = true)]
async fn starttls_not_advertised_without_tls() {
    let mut c = start(limits(), false);
    c.greet().await;
    let ehlo = c.ehlo().await;
    assert!(!ehlo.iter().any(|l| l.contains("STARTTLS")));
    assert_eq!(code(&c.cmd("STARTTLS").await), "502 5.5.1");
    c.quit().await;
}

#[tokio::test(start_paused = true)]
async fn starttls_injection_closes() {
    let mut c = start(limits(), true);
    c.greet().await;
    assert!(c.ehlo().await.contains(&"250-STARTTLS".to_owned()));
    c.send(b"STARTTLS\r\nMAIL FROM:<eve@example.com>\r\n").await;
    assert_eq!(code(&c.line().await), "554 5.5.1");
    let handler = c.closed().await;
    assert!(handler.rcpts.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn starttls_resets_the_session_and_requires_ehlo() {
    let mut c = start(limits(), true);
    c.greet().await;
    c.ehlo().await;
    c.cmd("MAIL FROM:<ann@example.com>").await;
    assert_eq!(code(&c.cmd("STARTTLS").await), "220 2.0.0");
    // The transaction and the EHLO are gone.
    assert_eq!(
        code(&c.cmd("MAIL FROM:<ann@example.com>").await),
        "503 5.5.1"
    );
    let ehlo = c.ehlo().await;
    assert!(!ehlo.iter().any(|l| l.contains("STARTTLS")));
    assert_eq!(code(&c.cmd("STARTTLS").await), "502 5.5.1");
    c.cmd("MAIL FROM:<ann@example.com>").await;
    c.cmd("RCPT TO:<a@orbit.test>").await;
    c.cmd("DATA").await;
    c.send(b"hi\r\n.\r\n").await;
    assert_eq!(code(&c.line().await), "250 2.0.0");
    let handler = c.quit().await;
    let messages = handler.messages.lock().unwrap();
    assert_eq!(messages[0].2.tls.as_ref().unwrap().version, "TLSv1.3");
}

#[tokio::test(start_paused = true)]
async fn size_declared_over_limit() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    assert_eq!(
        code(&c.cmd("MAIL FROM:<ann@example.com> SIZE=52428801").await),
        "552 5.3.4"
    );
    assert_eq!(code(&c.cmd("RCPT TO:<a@orbit.test>").await), "503 5.5.1");
    assert_eq!(
        code(&c.cmd("MAIL FROM:<ann@example.com> SIZE=52428800").await),
        "250 2.1.0"
    );
    c.quit().await;
}

#[tokio::test(start_paused = true)]
async fn size_stream_over_limit() {
    let mut c = start(
        SessionLimits {
            max_message_bytes: 1000,
            ..limits()
        },
        false,
    );
    c.greet().await;
    assert!(c.ehlo().await.contains(&"250-SIZE 1000".to_owned()));
    c.cmd("MAIL FROM:<ann@example.com>").await;
    c.cmd("RCPT TO:<a@orbit.test>").await;
    c.cmd("DATA").await;
    let line = format!("{}\r\n", "x".repeat(98));
    for _ in 0..50 {
        c.send(line.as_bytes()).await;
    }
    c.send(b".\r\n").await;
    assert_eq!(code(&c.line().await), "552 5.3.4");
    // The session goes on with a clean transaction.
    assert_eq!(code(&c.cmd("RCPT TO:<a@orbit.test>").await), "503 5.5.1");
    c.cmd("MAIL FROM:<ann@example.com>").await;
    c.cmd("RCPT TO:<a@orbit.test>").await;
    c.cmd("DATA").await;
    c.send(b"small\r\n.\r\n").await;
    assert_eq!(code(&c.line().await), "250 2.0.0");
    let handler = c.quit().await;
    let messages = handler.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].1, b"small\r\n");
}

#[tokio::test(start_paused = true)]
async fn data_line_over_limit() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    c.cmd("MAIL FROM:<ann@example.com>").await;
    c.cmd("RCPT TO:<a@orbit.test>").await;
    c.cmd("DATA").await;
    c.send(format!("{}\r\nmore\r\n.\r\n", "y".repeat(5000)).as_bytes())
        .await;
    assert_eq!(code(&c.line().await), "552 5.3.4");
    let handler = c.quit().await;
    assert!(handler.messages.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn recipient_cap_452() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    c.cmd("MAIL FROM:<ann@example.com>").await;
    for i in 0..100 {
        assert_eq!(
            code(&c.cmd(&format!("RCPT TO:<u{i}@orbit.test>")).await),
            "250 2.1.5"
        );
    }
    assert_eq!(code(&c.cmd("RCPT TO:<u100@orbit.test>").await), "452 4.5.3");
    c.cmd("DATA").await;
    c.send(b"hi\r\n.\r\n").await;
    assert_eq!(code(&c.line().await), "250 2.0.0");
    let handler = c.quit().await;
    assert_eq!(handler.messages.lock().unwrap()[0].0.recipients.len(), 100);
    assert_eq!(handler.rcpts.lock().unwrap().len(), 100);
}

#[tokio::test(start_paused = true)]
async fn failed_rcpt_tarpit_then_421() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    c.cmd("MAIL FROM:<ann@example.com>").await;
    for _ in 0..10 {
        let before = Instant::now();
        assert_eq!(
            code(&c.cmd("RCPT TO:<nobody@orbit.test>").await),
            "550 5.1.1"
        );
        assert!(before.elapsed() >= Duration::from_secs(1));
    }
    // A good recipient is not slowed down.
    let before = Instant::now();
    assert_eq!(code(&c.cmd("RCPT TO:<a@orbit.test>").await), "250 2.1.5");
    assert!(before.elapsed() < Duration::from_secs(1));
    assert_eq!(
        code(&c.cmd("RCPT TO:<x@elsewhere.example>").await),
        "421 4.7.0"
    );
    c.closed().await;
}

#[tokio::test(start_paused = true)]
async fn null_sender_one_recipient() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    assert_eq!(code(&c.cmd("MAIL FROM:<>").await), "250 2.1.0");
    assert_eq!(code(&c.cmd("RCPT TO:<a@orbit.test>").await), "250 2.1.5");
    assert_eq!(code(&c.cmd("RCPT TO:<b@orbit.test>").await), "452 4.5.3");
    c.cmd("DATA").await;
    c.send(b"bounce\r\n.\r\n").await;
    assert_eq!(code(&c.line().await), "250 2.0.0");
    let handler = c.quit().await;
    let messages = handler.messages.lock().unwrap();
    assert_eq!(messages[0].0.mail_from, None);
    assert_eq!(messages[0].0.recipients.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn early_talker() {
    let mut c = start(limits(), false);
    c.send(b"EHLO fast.example\r\n").await;
    let line = c.line().await;
    assert!(line.starts_with("554 "), "{line}");
    let handler = c.closed().await;
    assert!(handler.rcpts.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn greeting_waits_for_the_delay() {
    let before = Instant::now();
    let mut c = start(limits(), false);
    c.greet().await;
    assert!(before.elapsed() >= Duration::from_secs(1));
    c.quit().await;
}

#[tokio::test(start_paused = true)]
async fn long_command() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    let long = format!("NOOP {}", "z".repeat(600));
    assert_eq!(code(&c.cmd(&long).await), "500 5.5.2");
    // The rest of the long line was skipped; the session still works.
    assert_eq!(code(&c.cmd("NOOP").await), "250 2.0.0");
    // 512 bytes with CRLF is still allowed.
    let fits = format!("NOOP {}", "z".repeat(512 - 2 - 5));
    assert_eq!(code(&c.cmd(&fits).await), "250 2.0.0");
    c.quit().await;
}

#[tokio::test(start_paused = true)]
async fn command_timeout() {
    let mut c = start(limits(), false);
    c.greet().await;
    let before = Instant::now();
    assert_eq!(code(&c.line().await), "421 4.4.2");
    assert!(before.elapsed() >= Duration::from_secs(300));
    c.closed().await;
}

#[tokio::test(start_paused = true)]
async fn session_timeout() {
    let mut c = start(
        SessionLimits {
            session_timeout: Duration::from_secs(60),
            ..limits()
        },
        false,
    );
    c.greet().await;
    let before = Instant::now();
    assert_eq!(code(&c.line().await), "421 4.4.2");
    let waited = before.elapsed();
    assert!(waited >= Duration::from_secs(58) && waited < Duration::from_secs(61));
    c.closed().await;
}

#[tokio::test(start_paused = true)]
async fn shutdown_sends_421() {
    let mut c = start(limits(), false);
    c.greet().await;
    c.ehlo().await;
    c.cancel.cancel();
    assert_eq!(code(&c.line().await), "421 4.3.2");
    c.closed().await;
}
