//! Mail protocol code for Orbit: address rules, MIME parsing, the SPF, DKIM and DMARC checks and the inbound SMTP
//! session. Later modules add the HTML view document and DKIM keys. This crate has no database and no HTTP code.

mod address;
mod auth;
mod parse;
mod smtp;

pub use address::{
    Address, AddressError, MAX_ADDRESS_LEN, base_local, normalize_address, normalize_domain,
};
pub use auth::{
    AuthError, AuthOutcome, Authenticator, DmarcPolicy, DmarcVerdict, StaticDns, Verdict,
    authentication_results, strip_auth_results, trusted_relay_results,
};
pub use parse::{
    AttachmentInfo, Mailbox, ParseError, ParseLimits, ParsedMessage, normalize_subject, parse,
    part_bytes, sniff_image,
};
pub use smtp::{
    AcceptedRecipient, DataDecision, Envelope, MAX_COMMAND_LINE, MAX_TEXT_LINE, NoTls,
    RcptDecision, Reply, SessionContext, SessionError, SessionHandler, SessionLimits, StartTls,
    TARPIT_DELAY, TlsInfo, run_session,
};
