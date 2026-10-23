//! Address normalization for recipient lookup and domain claims.
//!
//! Orbit accepts only plain `local@domain` addresses. Forms that older relays used to route mail onward (source routes,
//! `%` and `!` paths, quoted local parts) are refused, so no address can make Orbit relay. Slice 1 has no `SMTPUTF8`, so
//! local parts are ASCII; domains may be Unicode and are stored as IDNA A-labels.

use thiserror::Error;

/// Longest address Orbit accepts (RFC 5321 path limit minus the angle brackets).
pub const MAX_ADDRESS_LEN: usize = 254;
const MAX_LOCAL_LEN: usize = 64;
const MAX_DOMAIN_LEN: usize = 253;
const MAX_LABEL_LEN: usize = 63;

/// A normalized address: lowercase local part and lowercase A-label domain without a trailing dot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Address {
    pub local: String,
    pub domain: String,
}

impl std::fmt::Display for Address {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}@{}", self.local, self.domain)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AddressError {
    #[error("the address is empty")]
    Empty,
    #[error("the address is too long")]
    TooLong,
    #[error("the address must have exactly one @")]
    AtSign,
    #[error("source routes are not accepted")]
    SourceRoute,
    #[error("quoted local parts are not accepted")]
    Quoted,
    #[error("% and ! routing is not accepted")]
    Routing,
    #[error("the local part is not valid")]
    InvalidLocal,
    #[error("the domain is not valid")]
    InvalidDomain,
}

/// Normalizes `local@domain`: lowercase, IDNA A-label domain, no trailing dot.
pub fn normalize_address(input: &str) -> Result<Address, AddressError> {
    if input.is_empty() {
        return Err(AddressError::Empty);
    }
    if input.len() > MAX_ADDRESS_LEN {
        return Err(AddressError::TooLong);
    }
    if input.starts_with('@') {
        return Err(AddressError::SourceRoute);
    }
    if input.starts_with('"') || input.contains('"') {
        return Err(AddressError::Quoted);
    }
    if input.contains('%') || input.contains('!') {
        return Err(AddressError::Routing);
    }
    let mut pieces = input.split('@');
    let (Some(local), Some(domain), None) = (pieces.next(), pieces.next(), pieces.next()) else {
        return Err(AddressError::AtSign);
    };
    let local = normalize_local(local)?;
    let domain = normalize_domain(domain)?;
    if local.len() + 1 + domain.len() > MAX_ADDRESS_LEN {
        return Err(AddressError::TooLong);
    }
    Ok(Address { local, domain })
}

/// Removes a `+tag` from a local part (`ann+news` → `ann`). A local part that starts with `+` is returned unchanged.
pub fn base_local(local: &str) -> &str {
    match local.find('+') {
        Some(index) if index > 0 => &local[..index],
        _ => local,
    }
}

/// Normalizes a domain name: lowercase IDNA A-labels, no trailing dot, letters, digits and inner hyphens only.
pub fn normalize_domain(input: &str) -> Result<String, AddressError> {
    let trimmed = input.strip_suffix('.').unwrap_or(input);
    if trimmed.is_empty() || trimmed.len() > MAX_DOMAIN_LEN * 4 {
        return Err(AddressError::InvalidDomain);
    }
    // `domain_to_ascii` maps case and Unicode equivalents and Punycode-encodes non-ASCII labels (UTS #46). It allows
    // any ASCII, so the host-name rules are checked below. https://docs.rs/idna/1.1.0/idna/fn.domain_to_ascii.html
    let ascii = idna::domain_to_ascii(trimmed).map_err(|_| AddressError::InvalidDomain)?;
    if ascii.is_empty() || ascii.len() > MAX_DOMAIN_LEN {
        return Err(AddressError::InvalidDomain);
    }
    for label in ascii.split('.') {
        let valid = !label.is_empty()
            && label.len() <= MAX_LABEL_LEN
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if !valid {
            return Err(AddressError::InvalidDomain);
        }
    }
    Ok(ascii)
}

/// A dot-atom local part (RFC 5322 §3.2.3) in ASCII, lowercased.
fn normalize_local(local: &str) -> Result<String, AddressError> {
    if local.is_empty()
        || local.len() > MAX_LOCAL_LEN
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local.bytes().all(is_local_byte)
    {
        return Err(AddressError::InvalidLocal);
    }
    Ok(local.to_ascii_lowercase())
}

fn is_local_byte(byte: u8) -> bool {
    // atext without `%` and `!` (refused earlier as routing), plus the dot.
    byte.is_ascii_alphanumeric() || b"#$&'*+-/=?^_`{|}~.".contains(&byte)
}
