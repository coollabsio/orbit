//! Notion import building blocks: an API client and a Notion → BlockNote converter.
//!
//! - [`client::NotionClient`]: typed, rate-limited reads (`Notion-Version: 2026-03-11`) and
//!   allowlisted file downloads.
//! - [`convert`]: pure conversion of a page's block tree into Orbit editor content.
//! - [`model`]: tolerant serde models shared by both.
//! - [`files`]: the Notion file host allowlist.
//! - [`scan`] and [`import`]: the import jobs, their state and the API service.

pub mod client;
pub mod convert;
pub mod files;
pub mod import;
pub mod model;
pub mod scan;

pub use client::{DownloadedFile, NotionClient, NotionClientConfig, NotionError};
pub use convert::{
    ConvertContext, ConvertOptions, ConvertReport, ConvertedPage, MapResolver, PageCover,
    ResolvedFile, Resolver,
};
pub use model::{
    Block, BlockNode, BlockTree, NotionDataSource, NotionDatabase, NotionFileRef, NotionPage,
    NotionUser, Parent, SearchKind, SearchResult,
};
