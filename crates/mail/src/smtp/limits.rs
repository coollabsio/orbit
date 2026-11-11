//! Limits of one SMTP session (spec §4.5).

use std::time::Duration;

/// Longest command line, CRLF included (RFC 5321 §4.5.3.1.4).
pub const MAX_COMMAND_LINE: usize = 512;

/// Longest line of message text, CRLF included. RFC 5321 allows 1000; real mail breaks that, so Orbit takes 4 KB.
pub const MAX_TEXT_LINE: usize = 4096;

/// The pause before each refused recipient answer, so address guessing is slow.
pub const TARPIT_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct SessionLimits {
    /// Largest raw message, after dot-unstuffing, CRLF line ends included. Advertised as `SIZE`.
    pub max_message_bytes: u64,
    pub max_recipients: usize,
    /// Refused recipients allowed in one session; the next one ends the session with `421 4.7.0`.
    pub max_failed_rcpt: usize,
    /// Wait for each command (RFC 5321 §4.5.3.2 gives 5 minutes).
    pub command_timeout: Duration,
    /// Wait for each read of message text (RFC 5321 §4.5.3.2 gives 3 minutes for a block, 10 for the end).
    pub data_timeout: Duration,
    /// Limit for the whole session.
    pub session_timeout: Duration,
    /// Silence asked of the client before the greeting; a client that talks first is refused.
    pub greeting_delay: Duration,
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: 52_428_800,
            max_recipients: 100,
            max_failed_rcpt: 10,
            command_timeout: Duration::from_secs(5 * 60),
            data_timeout: Duration::from_secs(10 * 60),
            session_timeout: Duration::from_secs(30 * 60),
            greeting_delay: Duration::from_secs(1),
        }
    }
}
