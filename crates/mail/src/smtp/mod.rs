//! The inbound SMTP session: a state machine over any `AsyncRead + AsyncWrite` stream, with the limits of spec §4.5.
//!
//! The session knows no database. It asks a [`SessionHandler`] about each recipient and hands it the finished
//! message; the server's listener implements the handler. Every reply text lives in [`Reply`].

mod limits;
mod session;

use std::fmt;
use std::future::Future;
use std::io;
use std::net::IpAddr;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};

pub use limits::{MAX_COMMAND_LINE, MAX_TEXT_LINE, SessionLimits, TARPIT_DELAY};
pub use session::run_session;

/// One SMTP reply: `code enhanced text`. An empty `enhanced` writes no enhanced status code (RFC 2034 leaves it out
/// of the greeting, the EHLO answer and `354`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reply {
    pub code: u16,
    pub enhanced: &'static str,
    pub text: &'static str,
}

impl Reply {
    const fn new(code: u16, enhanced: &'static str, text: &'static str) -> Self {
        Self {
            code,
            enhanced,
            text,
        }
    }

    // Spec §9 (and the plan's quota deviation).
    pub const USER_UNKNOWN: Self = Self::new(550, "5.1.1", "No such recipient here");
    pub const RELAY_DENIED: Self = Self::new(554, "5.7.1", "Relay access denied");
    pub const TOO_MANY_RECIPIENTS: Self = Self::new(452, "4.5.3", "Too many recipients");
    pub const NULL_SENDER_ONE_RECIPIENT: Self =
        Self::new(452, "4.5.3", "A null sender message takes one recipient");
    pub const TOO_MANY_FAILURES: Self = Self::new(
        421,
        "4.7.0",
        "Too many refused recipients, closing connection",
    );
    pub const MESSAGE_TOO_BIG: Self = Self::new(552, "5.3.4", "Message too big");
    pub const LINE_TOO_LONG: Self = Self::new(552, "5.3.4", "Message line too long");
    pub const BARE_NEWLINE: Self =
        Self::new(550, "5.6.0", "Bare CR or LF is not allowed, use CRLF");
    pub const STARTTLS_PIPELINED: Self = Self::new(
        554,
        "5.5.1",
        "Data sent before the TLS handshake, closing connection",
    );
    pub const DNS_TEMPORARY: Self = Self::new(
        451,
        "4.4.3",
        "Temporary DNS error while checking the sender, try again later",
    );
    pub const STORAGE_TEMPORARY: Self = Self::new(
        451,
        "4.3.0",
        "Temporary error while storing the message, try again later",
    );
    pub const QUOTA_EXCEEDED: Self = Self::new(452, "4.2.2", "Workspace storage is full");
    pub const DOMAIN_UNAVAILABLE: Self =
        Self::new(550, "5.7.1", "Mail for this domain is not accepted");
    pub const MAILBOX_SUSPENDED: Self = Self::new(550, "5.2.1", "Mailbox is disabled");
    pub const SHUTTING_DOWN: Self =
        Self::new(421, "4.3.2", "Server shutting down, try again later");

    // Session rules (spec §4.5).
    pub const EARLY_TALKER: Self = Self::new(
        554,
        "5.5.0",
        "Data sent before the greeting, closing connection",
    );
    pub const COMMAND_TOO_LONG: Self = Self::new(500, "5.5.2", "Command line too long");
    pub const TIMEOUT: Self = Self::new(421, "4.4.2", "Timeout, closing connection");

    // Protocol.
    pub const OK: Self = Self::new(250, "2.0.0", "OK");
    pub const SENDER_OK: Self = Self::new(250, "2.1.0", "Sender OK");
    pub const RECIPIENT_OK: Self = Self::new(250, "2.1.5", "Recipient OK");
    pub const START_DATA: Self = Self::new(354, "", "End data with <CR><LF>.<CR><LF>");
    pub const MESSAGE_ACCEPTED: Self = Self::new(250, "2.0.0", "Message accepted");
    pub const BYE: Self = Self::new(221, "2.0.0", "Bye");
    pub const READY_FOR_TLS: Self = Self::new(220, "2.0.0", "Ready to start TLS");
    pub const CANNOT_VRFY: Self = Self::new(252, "2.1.5", "Cannot verify the user");
    pub const EHLO_FIRST: Self = Self::new(503, "5.5.1", "Send EHLO first");
    pub const NESTED_MAIL: Self = Self::new(503, "5.5.1", "Sender already given");
    pub const MAIL_FIRST: Self = Self::new(503, "5.5.1", "Send MAIL first");
    pub const NO_RECIPIENTS: Self = Self::new(503, "5.5.1", "No valid recipients");
    pub const NOT_IMPLEMENTED: Self = Self::new(502, "5.5.1", "Command not implemented");
    pub const UNKNOWN_COMMAND: Self = Self::new(500, "5.5.1", "Unknown command");
    pub const SYNTAX_ERROR: Self = Self::new(501, "5.5.4", "Syntax error");
    pub const BAD_SENDER: Self = Self::new(501, "5.1.7", "Bad sender address syntax");
    pub const BAD_RECIPIENT: Self = Self::new(501, "5.1.3", "Bad recipient address syntax");
    pub const UNSUPPORTED_PARAMETER: Self = Self::new(555, "5.5.4", "Parameter not supported");

    /// Appends the reply line with CRLF.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.to_string().as_bytes());
        out.extend_from_slice(b"\r\n");
    }
}

impl fmt::Display for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.enhanced.is_empty() {
            write!(f, "{} {}", self.code, self.text)
        } else {
            write!(f, "{} {} {}", self.code, self.enhanced, self.text)
        }
    }
}

/// The negotiated TLS parameters, recorded with the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsInfo {
    pub version: String,
    pub cipher: String,
}

/// What the handler knows about the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    pub peer_ip: IpAddr,
    /// The name from the last `EHLO`/`HELO`; reset by `STARTTLS`.
    pub helo: Option<String>,
    pub tls: Option<TlsInfo>,
}

/// A recipient the handler accepted. The ids are opaque to this crate; the server maps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedRecipient {
    pub address_id: String,
    pub workspace_id: String,
    /// The forward-path as the client sent it.
    pub rcpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// `None` for the null sender `<>`.
    pub mail_from: Option<String>,
    pub recipients: Vec<AcceptedRecipient>,
    /// `SIZE=` from `MAIL FROM`, when given.
    pub declared_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RcptDecision {
    Accept(AcceptedRecipient),
    Reject(Reply),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataDecision {
    Accepted,
    Reject(Reply),
}

/// The server side of a session: recipient lookup and message intake.
pub trait SessionHandler: Send + Sync {
    /// Decides one `RCPT TO`. `rcpt` is the raw forward-path between `<` and `>` (source routes, quotes and `%`
    /// included), so the handler's address rules see exactly what the client sent.
    fn rcpt(&self, ctx: &SessionContext, rcpt: &str) -> impl Future<Output = RcptDecision> + Send;

    /// Takes a complete message (dot-unstuffed, CRLF line ends). Reply `250` only once it is stored durably.
    fn data(
        &self,
        ctx: &SessionContext,
        envelope: &Envelope,
        raw: Vec<u8>,
    ) -> impl Future<Output = DataDecision> + Send;
}

/// The TLS upgrade behind `STARTTLS`. The listener's rustls acceptor implements it; `STARTTLS` is advertised only
/// when `run_session` gets one.
pub trait StartTls: Send + Sync {
    type Stream<S>: AsyncRead + AsyncWrite + Unpin + Send
    where
        S: AsyncRead + AsyncWrite + Unpin + Send;

    /// Runs the server handshake on `stream`.
    fn accept<S>(
        &self,
        stream: S,
    ) -> impl Future<Output = io::Result<(Self::Stream<S>, TlsInfo)>> + Send
    where
        S: AsyncRead + AsyncWrite + Unpin + Send;
}

/// No TLS: pass `None::<&NoTls>` to `run_session`.
pub enum NoTls {}

impl StartTls for NoTls {
    type Stream<S>
        = S
    where
        S: AsyncRead + AsyncWrite + Unpin + Send;

    async fn accept<S>(&self, _stream: S) -> io::Result<(S, TlsInfo)>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        match *self {}
    }
}

/// Why a session ended abnormally. Protocol endings (QUIT, timeouts, refusals, shutdown) are `Ok`.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("connection error: {0}")]
    Io(#[from] io::Error),
    #[error("TLS handshake failed: {0}")]
    Tls(io::Error),
}
