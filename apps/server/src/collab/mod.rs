//! Real-time co-editing of docs pages (design: `.ai/docs-next/coediting-design.md`).
//!
//! - [`hub`]: one in-memory Yjs document ("room") per open page, y-sync fan-out, awareness,
//!   write-behind persistence (update log + compaction), the JSON projection into
//!   `pages.content_json`, and server-side replacements (restore, import, API content writes).
//! - [`socket`]: the WebSocket route `GET /api/v1/workspaces/{ws}/pages/{id}/collab`.
//! - [`blocknote`]: BlockNote JSON <-> Yjs converter (schema table generated from the editor).
//! - [`sanitize`]: the editor's link / file URL policy, for JSON and for live documents.
//!
//! Close codes (4400-4499 are terminal for y-websocket, the client does not reconnect by itself):
//! [`close`].

pub mod blocknote;
mod hub;
pub mod sanitize;
pub mod socket;

pub use hub::{CollabConfig, CollabError, CollabHub, CollabWrite};

/// WebSocket close codes of the collaboration socket.
pub mod close {
    /// A frame is not y-sync (malformed update or message).
    pub const BAD_MESSAGE: u16 = 4400;
    /// The session ended (logout, revocation, expiry, suspension): sign in again.
    pub const SESSION: u16 = 4401;
    /// No access any more (moved into someone's private space, removed from the workspace).
    pub const FORBIDDEN: u16 = 4403;
    /// The page was trashed or deleted.
    pub const GONE: u16 = 4404;
    /// The stored document was reset (backup restore, converter change) or the client's `epoch`
    /// is stale: drop the local document, fetch the page again and reconnect.
    pub const RESET: u16 = 4409;
    /// A frame or the document would exceed the size limits.
    pub const TOO_LARGE: u16 = 4413;
    /// Unsupported protocol version (`v` query parameter).
    pub const PROTOCOL: u16 = 4426;
    /// Too many messages per second.
    pub const RATE_LIMITED: u16 = 4429;
    /// The server is shutting down (transient; reconnect).
    pub const RESTART: u16 = 1012;
    /// Too many connections, memory budget exceeded, or a lagging connection (transient).
    pub const OVERLOADED: u16 = 1013;
}

/// The only protocol version (`?v=1`): y-sync v1 frames as y-websocket 3 sends them.
pub const PROTOCOL_VERSION: &str = "1";
