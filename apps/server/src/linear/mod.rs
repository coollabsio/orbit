//! Linear import: copies Linear teams into Orbit Tasks with their workflow states, issues,
//! sub-issues, labels, comments, uploaded files and relations.
//!
//! - [`client::LinearClient`]: typed, rate-limited GraphQL reads and authenticated upload
//!   downloads.
//! - [`model`]: tolerant serde models and the field selections.

pub mod client;
pub mod model;
