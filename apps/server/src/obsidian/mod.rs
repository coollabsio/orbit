//! Obsidian import: a vault uploaded as a zip becomes Docs pages.
//!
//! - [`vault`]: safe reading of the untrusted archive and Obsidian's link resolution.
//! - [`scan`]: the page tree the member picks from.
//! - [`archive`]: where the upload waits for its jobs.
//! - [`import`]: the import jobs, their state and the API service.

pub mod archive;
pub mod import;
pub mod scan;
pub mod vault;
