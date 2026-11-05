//! Obsidian import: a vault uploaded as a zip becomes Docs pages.
//!
//! - [`vault`]: safe reading of the untrusted archive and Obsidian's link resolution.
//! - [`scan`]: the page tree the member picks from.
//! - [`archive`]: where the upload waits for its jobs.

pub mod archive;
pub mod scan;
pub mod vault;
