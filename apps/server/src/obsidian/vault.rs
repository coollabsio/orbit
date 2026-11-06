//! Safe reading of an uploaded vault zip. Nothing is extracted to disk: entries are listed from
//! the archive, their names normalized, and files read into memory one at a time with their real
//! size checked against the limits (a zip header can lie about it).
//!
//! A path with `..`, an absolute path, a symbolic link, an encrypted entry, too many entries,
//! too many bytes in total or a suspicious compression ratio fails the whole archive with a
//! message for the member. Obsidian's own folders (`.obsidian/`, `.trash/`), `__MACOSX/`,
//! dotfiles, canvases and CSV files are skipped and counted, as are files over the per-file
//! limit and case-insensitive duplicates.

use std::collections::HashMap;
use std::io::{Read, Seek};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use zip::ZipArchive;

/// Limits for one archive.
#[derive(Clone, Copy, Debug)]
pub struct VaultLimits {
    pub max_entries: usize,
    pub max_total_bytes: u64,
    /// Larger files are skipped (and counted).
    pub max_file_bytes: u64,
    /// An entry that expands more than this many times (and to more than 1 MiB) is refused.
    pub max_ratio: u64,
}

impl VaultLimits {
    #[must_use]
    pub const fn new(max_file_bytes: u64) -> Self {
        Self {
            max_entries: 10_000,
            max_total_bytes: 1024 * 1024 * 1024,
            max_file_bytes,
            max_ratio: 100,
        }
    }
}

/// Entries smaller than this are not checked for their compression ratio (tiny text files
/// compress very well).
const RATIO_FLOOR: u64 = 1024 * 1024;

/// Why an archive cannot be imported. The message is shown to the member as is.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum VaultError {
    #[error("The file is not a valid zip archive.")]
    NotZip,
    #[error("The archive has more than {0} entries.")]
    TooManyEntries(usize),
    #[error("The archive contains an unsafe path: {0}")]
    UnsafePath(String),
    #[error("The archive contains a symbolic link: {0}")]
    Symlink(String),
    #[error("The archive is password protected.")]
    Encrypted,
    #[error("The archive unpacks to more than {0} MB.")]
    TooLarge(u64),
    #[error("The archive looks like a zip bomb: {0} expands too much.")]
    Bomb(String),
    #[error("The archive has no Markdown notes.")]
    Empty,
    #[error("The archive could not be read: {0}")]
    Read(String),
}

/// Files the scan skipped, by reason.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
pub struct ObsidianSkippedFiles {
    /// `.obsidian/`, `.trash/`, `__MACOSX/` and dotfiles.
    pub system: u32,
    /// Obsidian canvases (`.canvas`).
    pub canvas: u32,
    /// CSV files.
    pub csv: u32,
    /// Files no note links to or embeds.
    pub unreferenced: u32,
    /// Files over the upload size limit.
    pub too_large: u32,
    /// Files whose path differs from another file's only in letter case.
    pub duplicates: u32,
}

/// A file kept from the archive. `path` is normalized (`/`-separated, the single top-level
/// folder stripped).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultFile {
    pub path: String,
    /// The entry's index in the archive.
    pub index: usize,
    pub size: u64,
}

impl VaultFile {
    /// The file name without its folder.
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// What a link inside the vault points to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolved<'a> {
    Note(&'a VaultFile),
    Attachment(&'a VaultFile),
}

/// The notes and attachments of an archive, with Obsidian's link resolution.
#[derive(Clone, Debug, Default)]
pub struct Vault {
    pub notes: Vec<VaultFile>,
    pub attachments: Vec<VaultFile>,
    pub skipped: ObsidianSkippedFiles,
    /// Lowercase path → (is note, index into `notes` or `attachments`).
    by_path: HashMap<String, (bool, usize)>,
}

/// Whether a path is a Markdown note.
#[must_use]
pub fn is_note(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// A note's title: its file name without the extension (Obsidian's rule).
#[must_use]
pub fn note_title(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_lowercase();
    for extension in [".markdown", ".md"] {
        if lower.ends_with(extension) {
            return &name[..name.len() - extension.len()];
        }
    }
    name
}

/// A zip entry name as `/`-separated components, or why it is refused. Empty and `.`
/// components are dropped.
fn normalize(raw: &str) -> Result<Vec<&str>, VaultError> {
    let unsafe_path = || VaultError::UnsafePath(raw.chars().take(200).collect());
    if raw.starts_with('/') || raw.starts_with('\\') || raw.contains('\0') {
        return Err(unsafe_path());
    }
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(unsafe_path()),
            // A drive letter (`C:`) makes the path absolute on Windows.
            _ if parts.is_empty() && part.len() == 2 && part.ends_with(':') => {
                return Err(unsafe_path());
            }
            _ => parts.push(part),
        }
    }
    Ok(parts)
}

enum Skip {
    System,
    Canvas,
    Csv,
}

fn skip_reason(parts: &[&str]) -> Option<Skip> {
    if parts
        .iter()
        .any(|part| part.starts_with('.') || *part == "__MACOSX")
    {
        return Some(Skip::System);
    }
    let name = parts.last()?.to_lowercase();
    if name.ends_with(".canvas") {
        Some(Skip::Canvas)
    } else if name.ends_with(".csv") {
        Some(Skip::Csv)
    } else {
        None
    }
}

impl Vault {
    /// Lists an archive's files and checks every entry against `limits`.
    pub fn open<R: Read + Seek>(
        archive: &mut ZipArchive<R>,
        limits: VaultLimits,
    ) -> Result<Self, VaultError> {
        if archive.len() > limits.max_entries {
            return Err(VaultError::TooManyEntries(limits.max_entries));
        }
        struct Entry {
            parts: Vec<String>,
            index: usize,
            size: u64,
        }
        let mut entries = Vec::new();
        let mut total = 0u64;
        for index in 0..archive.len() {
            let file = archive.by_index_raw(index).map_err(read_error)?;
            let name = file
                .name()
                .map_err(|_| VaultError::UnsafePath("(invalid name)".to_owned()))?
                .into_owned();
            let parts = normalize(&name)?;
            if file.is_symlink() {
                return Err(VaultError::Symlink(name.chars().take(200).collect()));
            }
            if file.encrypted() {
                return Err(VaultError::Encrypted);
            }
            if file.is_dir() || parts.is_empty() {
                continue;
            }
            let size = file.size();
            total = total.saturating_add(size);
            if total > limits.max_total_bytes {
                return Err(VaultError::TooLarge(limits.max_total_bytes / 1_000_000));
            }
            if size > RATIO_FLOOR && size / file.compressed_size().max(1) > limits.max_ratio {
                return Err(VaultError::Bomb(name.chars().take(200).collect()));
            }
            entries.push(Entry {
                parts: parts.into_iter().map(str::to_owned).collect(),
                index,
                size,
            });
        }

        // One folder around everything (a zipped vault folder) is not part of the vault.
        let wrapper = entries
            .iter()
            .filter(|entry| entry.parts[0] != "__MACOSX")
            .map(|entry| (entry.parts.len() > 1).then_some(entry.parts[0].as_str()))
            .try_fold(None::<&str>, |common, first| match (common, first?) {
                (None, first) => Some(Some(first)),
                (Some(common), first) if common == first => Some(Some(common)),
                _ => None,
            })
            .flatten()
            .map(str::to_owned);

        let mut vault = Self::default();
        for entry in entries {
            let mut parts: Vec<&str> = entry.parts.iter().map(String::as_str).collect();
            if wrapper.as_deref() == Some(parts[0]) {
                parts.remove(0);
            }
            match skip_reason(&parts) {
                Some(Skip::System) => {
                    vault.skipped.system += 1;
                    continue;
                }
                Some(Skip::Canvas) => {
                    vault.skipped.canvas += 1;
                    continue;
                }
                Some(Skip::Csv) => {
                    vault.skipped.csv += 1;
                    continue;
                }
                None => {}
            }
            if entry.size > limits.max_file_bytes {
                vault.skipped.too_large += 1;
                continue;
            }
            let path = parts.join("/");
            let key = path.to_lowercase();
            if vault.by_path.contains_key(&key) {
                vault.skipped.duplicates += 1;
                continue;
            }
            let file = VaultFile {
                path,
                index: entry.index,
                size: entry.size,
            };
            if is_note(&file.path) {
                vault.by_path.insert(key, (true, vault.notes.len()));
                vault.notes.push(file);
            } else {
                vault.by_path.insert(key, (false, vault.attachments.len()));
                vault.attachments.push(file);
            }
        }
        if vault.notes.is_empty() {
            return Err(VaultError::Empty);
        }
        Ok(vault)
    }

    fn get(&self, key: &str) -> Option<Resolved<'_>> {
        self.by_path.get(key).map(|&(note, index)| {
            if note {
                Resolved::Note(&self.notes[index])
            } else {
                Resolved::Attachment(&self.attachments[index])
            }
        })
    }

    /// Resolves a link target the way Obsidian does: a path relative to the linking note, a path
    /// from the vault root, else the only file whose path ends with the target (by file name,
    /// letter case ignored). `Note#Heading` links to `Note`; `#Heading` to the note itself; a
    /// target without an extension also matches `.md` notes.
    #[must_use]
    pub fn resolve(&self, from: &str, target: &str) -> Option<Resolved<'_>> {
        let path = target.split(['#', '|']).next().unwrap_or_default().trim();
        if path.is_empty() {
            return target
                .starts_with('#')
                .then(|| self.get(&from.to_lowercase()))
                .flatten();
        }
        let target = path.replace('\\', "/").to_lowercase();
        let mut candidates = vec![target.clone()];
        if !is_note(&target) {
            candidates.push(format!("{target}.md"));
        }
        let folder: Vec<&str> = from
            .rsplit_once('/')
            .map(|(folder, _)| folder.split('/').collect())
            .unwrap_or_default();
        for candidate in &candidates {
            if let Some(relative) = join(&folder, candidate)
                && let Some(found) = self.get(&relative.to_lowercase())
            {
                return Some(found);
            }
            if let Some(found) = join(&[], candidate).and_then(|path| self.get(&path)) {
                return Some(found);
            }
        }
        for candidate in &candidates {
            let Some(candidate) = join(&[], candidate) else {
                continue;
            };
            let suffix = format!("/{candidate}");
            let mut matches = self
                .by_path
                .keys()
                .filter(|path| **path == candidate || path.ends_with(&suffix));
            if let (Some(only), None) = (matches.next(), matches.next()) {
                return self.get(only);
            }
        }
        None
    }

    /// Reads one file, refusing more bytes than its header declared.
    pub fn read<R: Read + Seek>(
        archive: &mut ZipArchive<R>,
        file: &VaultFile,
    ) -> Result<Vec<u8>, VaultError> {
        let entry = archive.by_index(file.index).map_err(read_error)?;
        let mut bytes = Vec::with_capacity(usize::try_from(file.size).unwrap_or(0).min(1 << 20));
        entry
            .take(file.size.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| VaultError::Read(error.to_string()))?;
        if bytes.len() as u64 > file.size {
            return Err(VaultError::Bomb(file.path.chars().take(200).collect()));
        }
        Ok(bytes)
    }
}

/// `folder` + `target` (with `./` and `..` resolved), or `None` when it leaves the vault.
fn join(folder: &[&str], target: &str) -> Option<String> {
    let mut parts: Vec<&str> = folder.to_vec();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/").to_lowercase())
}

fn read_error(error: zip::result::ZipError) -> VaultError {
    match error {
        zip::result::ZipError::InvalidArchive(_) | zip::result::ZipError::UnsupportedArchive(_) => {
            VaultError::NotZip
        }
        zip::result::ZipError::InvalidPassword => VaultError::Encrypted,
        other => VaultError::Read(other.to_string()),
    }
}

/// Opens an archive.
pub fn open_archive<R: Read + Seek>(reader: R) -> Result<ZipArchive<R>, VaultError> {
    ZipArchive::new(reader).map_err(read_error)
}
