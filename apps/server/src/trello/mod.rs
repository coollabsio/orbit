//! Trello import: copies Trello boards into Orbit Tasks with their lists, cards, labels,
//! checklists, comments and uploaded files.
//!
//! - [`client::TrelloClient`]: typed, rate-limited REST reads and authenticated attachment
//!   downloads.
//! - [`model`]: tolerant serde models and the field lists.
//! - [`scan`] and [`import`]: the import jobs, their state and the API service.

pub mod client;
pub mod import;
pub mod model;
pub mod scan;
