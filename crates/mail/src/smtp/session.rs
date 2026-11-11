//! The session loop. Lines are read by [`read_capped`], which never buffers more than its cap and refuses bare CR
//! and bare LF (the SMTP smuggling forms). `smtp-proto` only parses a complete, checked command line; message text
//! is read here, so only `CRLF.CRLF` can end it.

use std::io;
use std::time::Duration;

use smtp_proto::{Error as ProtoError, MAIL_BODY_BINARYMIME, MAIL_SMTPUTF8, Request};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::time::{Instant, sleep, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use super::limits::{MAX_COMMAND_LINE, MAX_TEXT_LINE, TARPIT_DELAY};
use super::{
    DataDecision, Envelope, RcptDecision, Reply, SessionContext, SessionError, SessionHandler,
    SessionLimits, StartTls,
};

/// Time allowed for the last reply and the close after the session has ended (a timeout may already be past).
const CLOSE_GRACE: Duration = Duration::from_secs(10);

/// Serves one SMTP connection until `QUIT`, a refusal that closes it, a timeout or `cancel` (then `421 4.3.2`).
///
/// `STARTTLS` is advertised only when `tls` is `Some`; pass `None::<&NoTls>` for a plain listener. After the
/// handshake the session starts over and the client must send `EHLO` again.
pub async fn run_session<S, H, T>(
    stream: S,
    mut ctx: SessionContext,
    handler: &H,
    limits: &SessionLimits,
    hostname: &str,
    tls: Option<&T>,
    cancel: &CancellationToken,
) -> Result<(), SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
    H: SessionHandler,
    T: StartTls,
{
    let deadline = Instant::now() + limits.session_timeout;
    let mut conn = Conn::new(stream, deadline, limits, cancel);

    let flow = match conn.greet(hostname).await {
        Ok(true) => serve(&mut conn, &mut ctx, handler, hostname, tls.is_some()).await,
        Ok(false) => Ok(Flow::Close),
        Err(stop) => Err(stop),
    };
    let Ok(Flow::StartTls) = flow else {
        return conn.finish(flow).await;
    };
    let Some(tls) = tls else {
        return conn.finish(Ok(Flow::Close)).await;
    };

    // `serve` checked that the read buffer is empty, so no plaintext byte crosses into the TLS session.
    let stream = conn.io.into_inner();
    let handshake = timeout_at(
        conn_deadline(deadline, limits.command_timeout),
        tls.accept(stream),
    );
    let (stream, info) = match handshake.await {
        Ok(Ok(upgraded)) => upgraded,
        Ok(Err(error)) => return Err(SessionError::Tls(error)),
        Err(_) => return Ok(()),
    };
    ctx.helo = None;
    ctx.tls = Some(info);
    let mut conn = Conn::new(stream, deadline, limits, cancel);
    let flow = serve(&mut conn, &mut ctx, handler, hostname, false).await;
    conn.finish(flow).await
}

/// How `serve` ended normally.
enum Flow {
    /// Flush the queued replies and close.
    Close,
    /// `220` was sent; run the TLS handshake.
    StartTls,
}

/// Why the session must end at once.
enum Stop {
    Eof,
    Timeout,
    Shutdown,
    Io(io::Error),
}

/// One line read by [`read_capped`]. The bytes (CRLF included) are in the caller's buffer for `Done`.
enum Line {
    Done,
    /// Longer than the cap; the rest of the line was read and dropped.
    TooLong,
    /// A CR not followed by LF, or an LF not preceded by CR.
    Bare,
}

/// How the message text ended.
enum Data {
    Complete(Vec<u8>),
    TooBig,
    LineTooLong,
    Bare,
}

fn conn_deadline(session_deadline: Instant, per_op: Duration) -> Instant {
    (Instant::now() + per_op).min(session_deadline)
}

struct Conn<'a, S> {
    io: BufReader<S>,
    /// Replies not yet written. They go out when no complete command is waiting (RFC 2920 batching).
    out: Vec<u8>,
    deadline: Instant,
    limits: &'a SessionLimits,
    cancel: &'a CancellationToken,
}

impl<'a, S: AsyncRead + AsyncWrite + Unpin + Send> Conn<'a, S> {
    fn new(
        stream: S,
        deadline: Instant,
        limits: &'a SessionLimits,
        cancel: &'a CancellationToken,
    ) -> Self {
        Self {
            io: BufReader::new(stream),
            out: Vec::new(),
            deadline,
            limits,
            cancel,
        }
    }

    fn reply(&mut self, reply: Reply) {
        reply.write_to(&mut self.out);
    }

    fn line(&mut self, text: &str) {
        self.out.extend_from_slice(text.as_bytes());
        self.out.extend_from_slice(b"\r\n");
    }

    async fn flush(&mut self, deadline: Instant) -> Result<(), Stop> {
        if self.out.is_empty() {
            return Ok(());
        }
        let out = std::mem::take(&mut self.out);
        let stream = self.io.get_mut();
        let write = async {
            stream.write_all(&out).await?;
            stream.flush().await
        };
        match timeout_at(deadline, write).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(Stop::Io(error)),
            Err(_) => Err(Stop::Timeout),
        }
    }

    /// Waits `greeting_delay` and sends the greeting. `Ok(false)`: the client talked first (`554` is queued).
    async fn greet(&mut self, hostname: &str) -> Result<bool, Stop> {
        let io = &mut self.io;
        let talked = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => return Err(Stop::Shutdown),
            read = timeout(self.limits.greeting_delay, async { io.fill_buf().await.map(|b| !b.is_empty()) }) => read,
        };
        match talked {
            Err(_) => {
                self.line(&format!("220 {hostname} ESMTP"));
                Ok(true)
            }
            Ok(Ok(true)) => {
                self.reply(Reply::EARLY_TALKER);
                Ok(false)
            }
            Ok(Ok(false)) => Err(Stop::Eof),
            Ok(Err(error)) => Err(Stop::Io(error)),
        }
    }

    /// Reads one line of at most `cap` bytes (CRLF included) into `buf`, waiting at most `per_op`.
    async fn read_line(
        &mut self,
        cap: usize,
        per_op: Duration,
        buf: &mut Vec<u8>,
    ) -> Result<Line, Stop> {
        let deadline = conn_deadline(self.deadline, per_op);
        if !self.io.buffer().contains(&b'\n') {
            self.flush(deadline).await?;
        }
        let io = &mut self.io;
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Stop::Shutdown),
            read = timeout_at(deadline, read_capped(io, cap, buf)) => match read {
                Ok(Ok(Some(line))) => Ok(line),
                Ok(Ok(None)) => Err(Stop::Eof),
                Ok(Err(error)) => Err(Stop::Io(error)),
                Err(_) => Err(Stop::Timeout),
            },
        }
    }

    /// Sleeps `delay` unless the server shuts down first.
    async fn pause(&self, delay: Duration) -> Result<(), Stop> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Stop::Shutdown),
            _ = sleep(delay) => Ok(()),
        }
    }

    /// Sends the last replies (with `421` for a timeout or shutdown) and closes the stream.
    async fn finish(&mut self, flow: Result<Flow, Stop>) -> Result<(), SessionError> {
        let result = match flow {
            Ok(_) | Err(Stop::Eof) => Ok(()),
            Err(Stop::Timeout) => {
                self.reply(Reply::TIMEOUT);
                Ok(())
            }
            Err(Stop::Shutdown) => {
                self.reply(Reply::SHUTTING_DOWN);
                Ok(())
            }
            Err(Stop::Io(error)) => return Err(error.into()),
        };
        let grace = Instant::now() + CLOSE_GRACE;
        match self.flush(grace).await {
            Ok(()) | Err(Stop::Timeout | Stop::Eof | Stop::Shutdown) => {}
            Err(Stop::Io(error)) => return Err(error.into()),
        }
        let _ = timeout_at(grace, self.io.get_mut().shutdown()).await;
        result
    }

    /// Reads message text up to `.` alone on a line, dot-unstuffing it. Over a limit it keeps reading to the end
    /// without keeping the text, so the session stays in step with the client.
    async fn read_data(&mut self) -> Result<Data, Stop> {
        let max = self.limits.max_message_bytes;
        let mut raw = Vec::new();
        let mut size: u64 = 0;
        let mut too_big = false;
        let mut too_long = false;
        let mut line = Vec::with_capacity(MAX_TEXT_LINE);
        loop {
            match self
                .read_line(MAX_TEXT_LINE, self.limits.data_timeout, &mut line)
                .await?
            {
                Line::Bare => return Ok(Data::Bare),
                Line::TooLong => {
                    too_long = true;
                    raw = Vec::new();
                    continue;
                }
                Line::Done => {}
            }
            if line == b".\r\n" {
                break;
            }
            let text = line.strip_prefix(b".").unwrap_or(&line);
            size += text.len() as u64;
            if size > max && !too_big {
                too_big = true;
                raw = Vec::new();
            }
            if !too_big && !too_long {
                raw.extend_from_slice(text);
            }
        }
        Ok(if too_long {
            Data::LineTooLong
        } else if too_big {
            Data::TooBig
        } else {
            Data::Complete(raw)
        })
    }
}

/// Reads one line ending in LF. Keeps at most `cap` bytes; a longer line is read to its end and dropped. Returns
/// `None` at end of stream. Stops at the first bare CR or bare LF.
async fn read_capped<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    cap: usize,
    buf: &mut Vec<u8>,
) -> io::Result<Option<Line>> {
    buf.clear();
    let mut len = 0usize;
    let mut prev = 0u8;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(None);
        }
        let mut bare = false;
        let mut end = None;
        for (i, &byte) in chunk.iter().enumerate() {
            if (prev == b'\r' && byte != b'\n') || (byte == b'\n' && prev != b'\r') {
                bare = true;
                end = Some(i + 1);
                break;
            }
            prev = byte;
            if byte == b'\n' {
                end = Some(i + 1);
                break;
            }
        }
        let take = end.unwrap_or(chunk.len());
        if len + take <= cap {
            buf.extend_from_slice(&chunk[..take]);
        }
        len += take;
        reader.consume(take);
        if bare {
            return Ok(Some(Line::Bare));
        }
        if end.is_some() {
            return Ok(Some(if len > cap { Line::TooLong } else { Line::Done }));
        }
    }
}

/// The raw forward-path of a `RCPT TO` line that `smtp-proto` accepted: the text between the first `<` and the next
/// `>`. A quoted `>` cuts it short, which still leaves a `"` for the address rules to refuse.
fn raw_path(line: &[u8]) -> String {
    let start = line.iter().position(|&b| b == b'<').map_or(0, |i| i + 1);
    let rest = &line[start..];
    let end = rest.iter().position(|&b| b == b'>').unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..end]).into_owned()
}

fn ehlo<S: AsyncRead + AsyncWrite + Unpin + Send>(
    conn: &mut Conn<'_, S>,
    hostname: &str,
    starttls: bool,
) {
    conn.line(&format!("250-{hostname}"));
    conn.line("250-PIPELINING");
    conn.line("250-8BITMIME");
    conn.line(&format!("250-SIZE {}", conn.limits.max_message_bytes));
    if starttls {
        conn.line("250-STARTTLS");
    }
    conn.line("250 ENHANCEDSTATUSCODES");
}

async fn serve<S, H>(
    conn: &mut Conn<'_, S>,
    ctx: &mut SessionContext,
    handler: &H,
    hostname: &str,
    starttls: bool,
) -> Result<Flow, Stop>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
    H: SessionHandler,
{
    let limits = conn.limits;
    let mut txn: Option<Envelope> = None;
    let mut failed_rcpt = 0usize;
    let mut buf = Vec::with_capacity(MAX_COMMAND_LINE);
    loop {
        match conn
            .read_line(MAX_COMMAND_LINE, limits.command_timeout, &mut buf)
            .await?
        {
            Line::Bare => {
                conn.reply(Reply::BARE_NEWLINE);
                return Ok(Flow::Close);
            }
            Line::TooLong => {
                conn.reply(Reply::COMMAND_TOO_LONG);
                continue;
            }
            Line::Done => {}
        }

        let request = match Request::parse(&mut buf.iter()) {
            Ok(request) => request.into_owned(),
            Err(error) => {
                let reply = match error {
                    ProtoError::UnknownCommand => Reply::UNKNOWN_COMMAND,
                    ProtoError::InvalidSenderAddress => Reply::BAD_SENDER,
                    ProtoError::InvalidRecipientAddress => Reply::BAD_RECIPIENT,
                    ProtoError::UnsupportedParameter { .. } => Reply::UNSUPPORTED_PARAMETER,
                    _ => Reply::SYNTAX_ERROR,
                };
                if reply == Reply::BAD_RECIPIENT && txn.is_some() {
                    failed_rcpt += 1;
                    if failed_rcpt > limits.max_failed_rcpt {
                        conn.reply(Reply::TOO_MANY_FAILURES);
                        return Ok(Flow::Close);
                    }
                    conn.pause(TARPIT_DELAY).await?;
                }
                conn.reply(reply);
                continue;
            }
        };

        match request {
            Request::Ehlo { host } => {
                ctx.helo = Some(host);
                txn = None;
                ehlo(conn, hostname, starttls);
            }
            Request::Helo { host } => {
                ctx.helo = Some(host);
                txn = None;
                conn.line(&format!("250 {hostname}"));
            }
            Request::Mail { from } => {
                if ctx.helo.is_none() {
                    conn.reply(Reply::EHLO_FIRST);
                } else if txn.is_some() {
                    conn.reply(Reply::NESTED_MAIL);
                } else if from.flags & (MAIL_SMTPUTF8 | MAIL_BODY_BINARYMIME) != 0 {
                    // Extensions that change the message format and are not advertised.
                    conn.reply(Reply::UNSUPPORTED_PARAMETER);
                } else if from.size as u64 > limits.max_message_bytes {
                    conn.reply(Reply::MESSAGE_TOO_BIG);
                } else {
                    txn = Some(Envelope {
                        mail_from: (!from.address.is_empty()).then_some(from.address),
                        recipients: Vec::new(),
                        declared_size: (from.size > 0).then_some(from.size as u64),
                    });
                    conn.reply(Reply::SENDER_OK);
                }
            }
            Request::Rcpt { .. } => {
                let Some(envelope) = txn.as_mut() else {
                    conn.reply(if ctx.helo.is_none() {
                        Reply::EHLO_FIRST
                    } else {
                        Reply::MAIL_FIRST
                    });
                    continue;
                };
                if envelope.mail_from.is_none() && !envelope.recipients.is_empty() {
                    conn.reply(Reply::NULL_SENDER_ONE_RECIPIENT);
                    continue;
                }
                if envelope.recipients.len() >= limits.max_recipients {
                    conn.reply(Reply::TOO_MANY_RECIPIENTS);
                    continue;
                }
                match handler.rcpt(ctx, &raw_path(&buf)).await {
                    RcptDecision::Accept(recipient) => {
                        envelope.recipients.push(recipient);
                        conn.reply(Reply::RECIPIENT_OK);
                    }
                    RcptDecision::Reject(reply) => {
                        failed_rcpt += 1;
                        if failed_rcpt > limits.max_failed_rcpt {
                            conn.reply(Reply::TOO_MANY_FAILURES);
                            return Ok(Flow::Close);
                        }
                        conn.pause(TARPIT_DELAY).await?;
                        conn.reply(reply);
                    }
                }
            }
            Request::Data => {
                let Some(envelope) = txn.take_if(|e| !e.recipients.is_empty()) else {
                    conn.reply(if ctx.helo.is_none() {
                        Reply::EHLO_FIRST
                    } else if txn.is_none() {
                        Reply::MAIL_FIRST
                    } else {
                        Reply::NO_RECIPIENTS
                    });
                    continue;
                };
                conn.reply(Reply::START_DATA);
                match conn.read_data().await? {
                    Data::Complete(raw) => match handler.data(ctx, &envelope, raw).await {
                        DataDecision::Accepted => conn.reply(Reply::MESSAGE_ACCEPTED),
                        DataDecision::Reject(reply) => conn.reply(reply),
                    },
                    Data::TooBig => conn.reply(Reply::MESSAGE_TOO_BIG),
                    Data::LineTooLong => conn.reply(Reply::LINE_TOO_LONG),
                    Data::Bare => {
                        conn.reply(Reply::BARE_NEWLINE);
                        return Ok(Flow::Close);
                    }
                }
            }
            Request::StartTls => {
                if !starttls {
                    conn.reply(Reply::NOT_IMPLEMENTED);
                } else if ctx.helo.is_none() {
                    conn.reply(Reply::EHLO_FIRST);
                } else if !conn.io.buffer().is_empty() {
                    // Commands pipelined after STARTTLS would run as if they came over TLS (CVE-2011-0411).
                    conn.reply(Reply::STARTTLS_PIPELINED);
                    return Ok(Flow::Close);
                } else {
                    conn.reply(Reply::READY_FOR_TLS);
                    conn.flush(conn_deadline(conn.deadline, limits.command_timeout))
                        .await?;
                    return Ok(Flow::StartTls);
                }
            }
            Request::Rset => {
                txn = None;
                conn.reply(Reply::OK);
            }
            Request::Noop { .. } => conn.reply(Reply::OK),
            Request::Vrfy { .. } => conn.reply(Reply::CANNOT_VRFY),
            Request::Quit => {
                conn.reply(Reply::BYE);
                return Ok(Flow::Close);
            }
            Request::Lhlo { .. }
            | Request::Bdat { .. }
            | Request::Auth { .. }
            | Request::Expn { .. }
            | Request::Help { .. }
            | Request::Etrn { .. }
            | Request::Atrn { .. }
            | Request::Burl { .. } => conn.reply(Reply::NOT_IMPLEMENTED),
        }
    }
}
