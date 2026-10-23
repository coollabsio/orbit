//! Mail protocol code for Orbit: address rules and MIME parsing. Later modules add the authentication checks, the
//! SMTP session, the HTML view document and DKIM keys. This crate has no database and no HTTP code.

mod address;
mod parse;

pub use address::{
    Address, AddressError, MAX_ADDRESS_LEN, base_local, normalize_address, normalize_domain,
};
pub use parse::{
    AttachmentInfo, Mailbox, ParseError, ParseLimits, ParsedMessage, normalize_subject, parse,
    part_bytes, sniff_image,
};
