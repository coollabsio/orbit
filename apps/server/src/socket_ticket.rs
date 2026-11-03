//! Socket tickets let a WebSocket that cannot send the session (a browser socket inside a native
//! app's WebView: no `Authorization` header, no cookie) open the co-editing socket. A ticket names
//! a session and one document (page or task), and expires 60 s after it was issued. It is
//! stateless: base64url (no padding) of `session id (36 ASCII bytes) || expiry (i64 Unix ms,
//! big-endian) || HMAC-SHA256`, keyed by a subkey of the app key. The HMAC also covers the
//! document (kind and id), which the socket URL names, so a leaked ticket opens no other document.
//! The session is still checked when the ticket is used.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use orbit_platform::{Id, TimestampMillis};
use sha2::Sha256;

use crate::collab::{DocKey, DocKind};

/// How long a ticket stays valid after it is issued.
pub const TICKET_LIFETIME_MS: i64 = 60_000;

const ID_LEN: usize = 36;
const PAYLOAD_LEN: usize = ID_LEN + 8;
const TICKET_LEN: usize = PAYLOAD_LEN + 32;

/// Issues and verifies socket tickets.
#[derive(Clone)]
pub struct SocketTickets {
    key: [u8; 32],
}

impl SocketTickets {
    /// Derives the ticket key from the server's app key (`ORBIT_APP_KEY`).
    #[must_use]
    pub fn new(app_key: &[u8; 32]) -> Self {
        let mut key = [0_u8; 32];
        Hkdf::<Sha256>::new(None, app_key)
            .expand(b"orbit socket ticket v1", &mut key)
            .expect("32 bytes is a valid HKDF-SHA256 output length");
        Self { key }
    }

    /// A ticket for `session` to open `document`, that expires at `expires_at`.
    #[must_use]
    pub fn issue(&self, session: Id, document: DocKey, expires_at: TimestampMillis) -> String {
        let mut ticket = Vec::with_capacity(TICKET_LEN);
        ticket.extend_from_slice(session.to_string().as_bytes());
        ticket.extend_from_slice(&expires_at.as_millis().to_be_bytes());
        let signature = self.mac(&ticket, document).finalize().into_bytes();
        ticket.extend_from_slice(&signature);
        URL_SAFE_NO_PAD.encode(ticket)
    }

    /// The session of a ticket this server signed for `document` that has not expired at `now`.
    #[must_use]
    pub fn verify(&self, ticket: &str, document: DocKey, now: TimestampMillis) -> Option<Id> {
        let bytes = URL_SAFE_NO_PAD.decode(ticket).ok()?;
        if bytes.len() != TICKET_LEN {
            return None;
        }
        let (payload, signature) = bytes.split_at(PAYLOAD_LEN);
        self.mac(payload, document).verify_slice(signature).ok()?;
        let (session, expires_at) = payload.split_at(ID_LEN);
        let expires_at = i64::from_be_bytes(expires_at.try_into().ok()?);
        if now.as_millis() >= expires_at {
            return None;
        }
        std::str::from_utf8(session).ok()?.parse().ok()
    }

    fn mac(&self, payload: &[u8], document: DocKey) -> Hmac<Sha256> {
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(&self.key).expect("HMAC accepts any key length");
        mac.update(payload);
        mac.update(match document.kind {
            DocKind::Page => b"p",
            DocKind::Task => b"t",
        });
        mac.update(document.id.to_string().as_bytes());
        mac
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use orbit_platform::{Id, TimestampMillis};

    use super::SocketTickets;
    use crate::collab::DocKey;

    const NOW: TimestampMillis = TimestampMillis::from_millis(1_700_000_000_000);
    const LATER: TimestampMillis = TimestampMillis::from_millis(1_700_000_060_000);

    #[test]
    fn a_ticket_names_its_session_until_it_expires() {
        let tickets = SocketTickets::new(&[1; 32]);
        let session = Id::new_v7();
        let page = DocKey::page(Id::new_v7());
        let ticket = tickets.issue(session, page, LATER);
        assert_eq!(ticket.len(), 102);
        assert!(
            ticket
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
        assert_eq!(tickets.verify(&ticket, page, NOW), Some(session));
        assert_eq!(
            tickets.verify(
                &ticket,
                page,
                TimestampMillis::from_millis(LATER.as_millis() - 1)
            ),
            Some(session)
        );
        assert_eq!(tickets.verify(&ticket, page, LATER), None);
    }

    #[test]
    fn a_ticket_opens_only_its_document() {
        let tickets = SocketTickets::new(&[1; 32]);
        let id = Id::new_v7();
        let ticket = tickets.issue(Id::new_v7(), DocKey::page(id), LATER);
        assert!(tickets.verify(&ticket, DocKey::page(id), NOW).is_some());
        assert_eq!(tickets.verify(&ticket, DocKey::task(id), NOW), None);
        assert_eq!(
            tickets.verify(&ticket, DocKey::page(Id::new_v7()), NOW),
            None
        );
    }

    #[test]
    fn altered_or_foreign_tickets_are_refused() {
        let tickets = SocketTickets::new(&[1; 32]);
        let page = DocKey::page(Id::new_v7());
        let ticket = tickets.issue(Id::new_v7(), page, LATER);
        assert_eq!(
            SocketTickets::new(&[2; 32]).verify(&ticket, page, NOW),
            None
        );

        let mut bytes = URL_SAFE_NO_PAD.decode(&ticket).unwrap();
        // A later expiry with the old signature.
        bytes[43] ^= 1;
        assert_eq!(
            tickets.verify(&URL_SAFE_NO_PAD.encode(&bytes), page, NOW),
            None
        );
        assert_eq!(tickets.verify(&ticket[..100], page, NOW), None);
        assert_eq!(tickets.verify(&format!("{ticket}A"), page, NOW), None);
        assert_eq!(tickets.verify("", page, NOW), None);
        assert_eq!(tickets.verify("not base64!", page, NOW), None);
    }
}
