//! Asana import: copies Asana projects into Orbit Tasks with their sections, tasks, subtasks,
//! comments, files and dependencies.
//!
//! - [`client::AsanaClient`]: typed, rate-limited reads and allowlisted file downloads.
//! - [`convert`]: Asana rich text to BlockNote blocks.
//! - [`model`]: tolerant serde models.
//! - [`scan`] and [`import`]: the import jobs, their state and the API service.

pub mod client;
pub mod convert;
pub mod import;
pub mod model;
pub mod scan;
