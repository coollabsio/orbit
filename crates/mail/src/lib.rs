//! Mail protocol code for Orbit: address rules, MIME parsing, the SPF, DKIM and DMARC checks and the inbound SMTP
//! session with its TLS acceptor, PROXY v2 and connection limits, the sanitized HTML view document and DKIM keys.
//! This crate has no database and no HTTP code.

mod address;
mod auth;
mod dkim;
mod html;
pub mod inbound;
mod parse;
mod smtp;

pub use address::{
    Address, AddressError, MAX_ADDRESS_LEN, base_local, normalize_address, normalize_domain,
};
pub use auth::{
    AuthError, AuthOutcome, Authenticator, DmarcPolicy, DmarcVerdict, ProviderSpf, SpfHeader,
    StaticDns, Verdict, authentication_results, provider_spf, strip_auth_results,
    trusted_relay_results,
};
pub use dkim::{DkimKey, dkim_txt, generate_dkim_key, new_selector};
pub use html::{
    CidUrl, ProxyUrl, RenderOptions, Rendered, csp_header, new_nonce, render_document,
    render_text_document,
};
/// The resolver crate `mail-auth` uses, so the server's DNS checks share its version and features.
pub use mail_auth::hickory_resolver;
pub use parse::{
    AttachmentInfo, Mailbox, ParseError, ParseLimits, ParsedMessage, normalize_subject, parse,
    part_bytes, sniff_image,
};
pub use smtp::{
    AcceptedRecipient, CERT_EXPIRY_WARNING, CertInfo, ConnLimiter, ConnPermit,
    DOCKER_DEFAULT_GATEWAY, DataDecision, Envelope, MAX_COMMAND_LINE, MAX_TEXT_LINE, NoTls,
    ProxyError, RcptDecision, Reply, SessionContext, SessionError, SessionHandler, SessionLimits,
    StartTls, TARPIT_DELAY, TlsAcceptorHandle, TlsError, TlsInfo, is_docker_bridge, read_proxy_v2,
    run_session,
};
