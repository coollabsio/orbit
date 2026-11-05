//! Trello import: copies Trello boards into Orbit Tasks with their lists, cards, labels,
//! checklists, comments and uploaded files.
//!
//! - [`client::TrelloClient`]: typed, rate-limited REST reads and authenticated attachment
//!   downloads.
//! - [`model`]: tolerant serde models and the field lists.

pub mod client;
pub mod model;
