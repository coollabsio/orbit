//! Page export: Markdown files in a ZIP archive.
//!
//! Layout (Notion-like): the root page is `<Title>.md`; a page's sub-pages live in the folder
//! `<Title>/` next to it and its files in `<Title>/assets/`. Names are sanitized, capped and
//! unique per folder (ignoring case). Links between exported pages are relative; links to other
//! pages and page files become absolute app URLs; external links stay.

pub mod markdown;
pub mod names;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Seek, SeekFrom, Write};

use orbit_platform::{Id, TimestampMillis};
use tokio::io::AsyncReadExt;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::repositories::page_export::{ExportFile, ExportSource, PageExportError};
use crate::repositories::page_files::{PageFileError, PageFileRepository, parse_page_file_url};
use markdown::{LinkResolver, PageLink, blocks_to_markdown, escape_label, link_destination};
use names::{
    ASSETS_DIR, NameSet, STEM_MAX_CHARS, relative_link, sanitize_segment, split_file_name,
};

/// Export size limits.
#[derive(Clone, Copy, Debug)]
pub struct ExportLimits {
    pub max_pages: usize,
    /// Uncompressed bytes (Markdown plus files).
    pub max_bytes: u64,
}

impl Default for ExportLimits {
    fn default() -> Self {
        Self {
            max_pages: 500,
            max_bytes: 200 * 1024 * 1024,
        }
    }
}

/// One file in the archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExportEntry {
    Markdown {
        path: String,
        text: String,
    },
    Asset {
        path: String,
        page_id: Id,
        file_id: Id,
        size_bytes: u64,
        compress: bool,
    },
}

impl ExportEntry {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Markdown { path, .. } | Self::Asset { path, .. } => path,
        }
    }
}

/// The archive to write: its entries in order and the download name (without `.zip`).
#[derive(Clone, Debug)]
pub struct ExportPlan {
    pub name: String,
    pub entries: Vec<ExportEntry>,
}

struct PlannedPage {
    /// Folder of the `.md` file.
    dir: Vec<String>,
    /// File-name stem; also the name of the folder with its sub-pages and assets.
    stem: String,
}

impl PlannedPage {
    fn md_path(&self) -> Vec<String> {
        let mut path = self.dir.clone();
        path.push(format!("{}.md", self.stem));
        path
    }

    fn own_dir(&self) -> Vec<String> {
        let mut path = self.dir.clone();
        path.push(self.stem.clone());
        path
    }
}

#[derive(Default)]
struct Assets {
    /// `(page, file)` → archive path segments.
    paths: HashMap<(Id, Id), Vec<String>>,
    /// Names used in each page's assets folder.
    names: HashMap<Id, NameSet>,
    order: Vec<(Id, Id)>,
}

struct Resolver<'a> {
    workspace_id: Id,
    origin: &'a str,
    source: &'a ExportSource,
    planned: &'a HashMap<Id, PlannedPage>,
    assets: &'a RefCell<Assets>,
    /// Folder of the page being converted.
    from_dir: &'a [String],
}

impl Resolver<'_> {
    fn absolute(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }

    fn asset(&self, file: &ExportFile) -> Vec<String> {
        let key = (file.page_id, file.id);
        let mut assets = self.assets.borrow_mut();
        if let Some(path) = assets.paths.get(&key) {
            return path.clone();
        }
        let (stem, extension) = split_file_name(&file.file_name);
        let name = assets
            .names
            .entry(file.page_id)
            .or_default()
            .claim(&stem, &[&extension]);
        let mut path = self.planned[&file.page_id].own_dir();
        path.push(ASSETS_DIR.to_owned());
        path.push(names::with_extension(&name, &extension));
        assets.paths.insert(key, path.clone());
        assets.order.push(key);
        path
    }
}

impl LinkResolver for Resolver<'_> {
    fn member_name(&self, user_id: &str) -> Option<String> {
        let id: Id = user_id.parse().ok()?;
        self.source.members.get(&id).cloned()
    }

    fn page(&self, page_id: &str) -> Option<PageLink> {
        let id: Id = page_id.parse().ok()?;
        let title = self.source.titles.get(&id)?;
        let href = match self.planned.get(&id) {
            Some(page) => relative_link(self.from_dir, &page.md_path()),
            None => self.absolute(&format!("/docs/{id}")),
        };
        Some(PageLink {
            title: title.clone(),
            href,
        })
    }

    fn url(&self, url: &str) -> String {
        if let Some((workspace, page, file)) = parse_page_file_url(url) {
            if workspace == self.workspace_id
                && self.planned.contains_key(&page)
                && let Some(file) = self.source.files.get(&(page, file))
            {
                return relative_link(self.from_dir, &self.asset(file));
            }
            return self.absolute(url);
        }
        if let Some(rest) = url.strip_prefix("/docs/") {
            let (id, _) = rest.split_once(['#', '?']).unwrap_or((rest, ""));
            if let Some(page) = id.parse::<Id>().ok().and_then(|id| self.planned.get(&id)) {
                return relative_link(self.from_dir, &page.md_path());
            }
            return self.absolute(url);
        }
        if url.starts_with('/') && !url.starts_with("//") {
            return self.absolute(url);
        }
        url.to_owned()
    }
}

/// Media types that are already compressed; stored as they are.
fn compressed_type(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "application/pdf"
            | "application/zip"
    ) || mime_type.starts_with("video/")
        || mime_type.starts_with("audio/")
}

/// YAML double-quoted scalar (JSON string syntax is valid YAML).
fn yaml_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// Lays out the archive and converts every page. `origin` is the app's public origin (no
/// trailing slash) for links that leave the export.
pub fn plan(
    source: &ExportSource,
    workspace_id: Id,
    origin: &str,
    exported_at: TimestampMillis,
    limits: ExportLimits,
) -> Result<ExportPlan, PageExportError> {
    let origin = origin.trim_end_matches('/');
    let root = source.pages.first().ok_or(PageExportError::NotFound)?;
    let mut planned: HashMap<Id, PlannedPage> = HashMap::new();
    // Names per folder, keyed by the page that owns the folder (`None` = archive root).
    let mut folders: HashMap<Option<Id>, NameSet> = HashMap::new();
    for page in &source.pages {
        let parent = if page.id == root.id {
            None
        } else {
            page.parent_id
        };
        let dir = match parent {
            None => Vec::new(),
            Some(parent) => planned
                .get(&parent)
                .map(PlannedPage::own_dir)
                .ok_or(PageExportError::Corrupt)?,
        };
        let names = folders.entry(parent).or_insert_with(|| {
            if parent.is_some() {
                NameSet::with_reserved(&[ASSETS_DIR])
            } else {
                NameSet::default()
            }
        });
        let stem = names.claim(
            &sanitize_segment(&page.title, STEM_MAX_CHARS, "Untitled"),
            &["md", ""],
        );
        planned.insert(page.id, PlannedPage { dir, stem });
    }

    let assets = RefCell::new(Assets::default());
    let mut entries = Vec::with_capacity(source.pages.len());
    let mut total: u64 = 0;
    for page in &source.pages {
        let layout = &planned[&page.id];
        let resolver = Resolver {
            workspace_id,
            origin,
            source,
            planned: &planned,
            assets: &assets,
            from_dir: &layout.dir,
        };
        let title = if page.title.trim().is_empty() {
            "Untitled"
        } else {
            page.title.trim()
        };
        let mut text = format!("---\ntitle: {}\n", yaml_string(title));
        if let Some(icon) = page.icon.as_deref().filter(|icon| !icon.trim().is_empty()) {
            text.push_str(&format!("icon: {}\n", yaml_string(icon)));
        }
        text.push_str(&format!(
            "exported_at: {}\n---\n\n",
            yaml_string(&exported_at.to_string())
        ));
        text.push_str(&format!("# {}\n", escape_label(title)));
        if let Some(cover) = page.cover_url.as_deref().filter(|cover| !cover.is_empty()) {
            text.push_str(&format!(
                "\n![Cover]({})\n",
                link_destination(&resolver.url(cover))
            ));
        }
        let body = blocks_to_markdown(&page.content, &resolver);
        if !body.is_empty() {
            text.push('\n');
            text.push_str(&body);
        }
        total = total.saturating_add(text.len() as u64);
        entries.push(ExportEntry::Markdown {
            path: layout.md_path().join("/"),
            text,
        });
    }
    let assets = assets.into_inner();
    for key in &assets.order {
        let file = &source.files[key];
        total = total.saturating_add(file.size_bytes);
        entries.push(ExportEntry::Asset {
            path: assets.paths[key].join("/"),
            page_id: file.page_id,
            file_id: file.id,
            size_bytes: file.size_bytes,
            compress: !compressed_type(&file.mime_type),
        });
    }
    if total > limits.max_bytes {
        return Err(PageExportError::TooLarge {
            limit: limits.max_bytes,
        });
    }
    Ok(ExportPlan {
        name: planned[&root.id].stem.clone(),
        entries,
    })
}

/// `(year, month, day, hour, minute, second)` in UTC.
fn civil(millis: i64) -> (i64, u32, u32, u32, u32, u32) {
    let seconds = millis.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    // Howard Hinnant's days-from-civil inverse.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (of_day / 3600) as u32,
        ((of_day % 3600) / 60) as u32,
        (of_day % 60) as u32,
    )
}

fn zip_time(at: TimestampMillis) -> zip::DateTime {
    let (year, month, day, hour, minute, second) = civil(at.as_millis());
    u16::try_from(year)
        .ok()
        .and_then(|year| {
            zip::DateTime::from_date_and_time(
                year,
                month as u8,
                day as u8,
                hour as u8,
                minute as u8,
                second as u8,
            )
            .ok()
        })
        .unwrap_or_default()
}

struct Chunk {
    path: String,
    bytes: Vec<u8>,
    compress: bool,
}

/// Writes the archive into an anonymous temporary file (deleted when closed) and returns it
/// rewound. File bytes are read one at a time through the page-file access checks.
pub async fn write_archive(
    plan: ExportPlan,
    files: &PageFileRepository,
    workspace_id: Id,
    actor_id: Id,
    exported_at: TimestampMillis,
    limits: ExportLimits,
) -> Result<std::fs::File, PageExportError> {
    let modified = zip_time(exported_at);
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Chunk>(1);
    let writer = tokio::task::spawn_blocking(move || -> std::io::Result<std::fs::File> {
        let mut zip = ZipWriter::new(tempfile::tempfile()?);
        while let Some(Chunk {
            path,
            bytes,
            compress,
        }) = receiver.blocking_recv()
        {
            let method = if compress {
                CompressionMethod::Deflated
            } else {
                CompressionMethod::Stored
            };
            let options = SimpleFileOptions::default()
                .compression_method(method)
                .last_modified_time(modified)
                .unix_permissions(0o644);
            zip.start_file(path, options)
                .map_err(std::io::Error::other)?;
            zip.write_all(&bytes)?;
        }
        let mut file = zip.finish().map_err(std::io::Error::other)?;
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    });

    let mut total: u64 = 0;
    let mut failure = None;
    for entry in plan.entries {
        let chunk = match entry {
            ExportEntry::Markdown { path, text } => Chunk {
                path,
                bytes: text.into_bytes(),
                compress: true,
            },
            ExportEntry::Asset {
                path,
                page_id,
                file_id,
                compress,
                ..
            } => {
                let bytes = match read_file(
                    files,
                    workspace_id,
                    page_id,
                    file_id,
                    actor_id,
                    limits.max_bytes.saturating_sub(total),
                )
                .await
                {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                };
                Chunk {
                    path,
                    bytes,
                    compress,
                }
            }
        };
        total = total.saturating_add(chunk.bytes.len() as u64);
        if total > limits.max_bytes {
            failure = Some(PageExportError::TooLarge {
                limit: limits.max_bytes,
            });
            break;
        }
        if sender.send(chunk).await.is_err() {
            break;
        }
    }
    drop(sender);
    let written = writer
        .await
        .map_err(|error| PageExportError::Io(error.to_string()))?
        .map_err(|error| PageExportError::Io(error.to_string()));
    match failure {
        Some(error) => Err(error),
        None => written,
    }
}

async fn read_file(
    files: &PageFileRepository,
    workspace_id: Id,
    page_id: Id,
    file_id: Id,
    actor_id: Id,
    budget: u64,
) -> Result<Vec<u8>, PageExportError> {
    let download = files
        .download(workspace_id, page_id, file_id, actor_id)
        .await
        .map_err(|error| match error {
            PageFileError::NotFound => PageExportError::NotFound,
            PageFileError::Database(error) => PageExportError::Unavailable(error),
            PageFileError::Upload(error) => PageExportError::Io(error.to_string()),
        })?;
    let mut bytes = Vec::new();
    download
        .reader
        .take(budget.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| PageExportError::Io(error.to_string()))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::civil;

    #[test]
    fn civil_dates_match_known_timestamps() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        // 2026-09-25T13:14:15Z
        assert_eq!(civil(1_790_342_055_000), (2026, 9, 25, 13, 14, 15));
        // 2024-02-29T23:59:59Z
        assert_eq!(civil(1_709_251_199_000), (2024, 2, 29, 23, 59, 59));
    }
}
