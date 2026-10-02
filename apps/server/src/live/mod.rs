//! The live socket of a workspace: one WebSocket for each open tab, which carries what changed
//! as it changes. Chat is its first topic.
//!
//! - [`hub`]: the connections of each workspace, in memory. It numbers the events, keeps the
//!   newest ones for a client that reconnects, and knows who is online.
//! - [`socket`]: the route `GET /api/v1/workspaces/{workspace_id}/live`.
//!
//! Frames from the server are JSON text:
//! - `{"type":"hello","epoch":…,"seq":…,"online":[user ids]}`: always the first frame.
//! - `{"seq":…,"topic":…,"event":{…}}`: an event. `seq` grows by one for each event of the
//!   workspace; a client only sees the events that are for it, so it sees gaps.
//! - `{"topic":…,"event":{…}}`: a signal that is not kept (typing, presence).
//! - `{"type":"resync"}`: the client missed events that the hub no longer has; it must fetch
//!   its data again.
//! - `{"type":"ping"}`: every 25 seconds, so the client can tell a dead connection.
//!
//! The hub lives in this process only, as the server does.

mod hub;
pub mod socket;

pub use hub::{LiveHub, Recipients};
