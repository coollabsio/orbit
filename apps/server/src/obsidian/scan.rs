//! The scan: reads every note of a vault once to find its links, and arranges folders and notes
//! as the page tree the member picks from.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zip::ZipArchive;

use super::vault::{ObsidianSkippedFiles, Resolved, Vault, VaultError, VaultLimits, note_title};
use crate::collab::markdown_import::obsidian_links;

/// A folder or a note in the scan tree.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObsidianImportNodeKind {
    /// A page named like the folder; its folders and notes become sub-pages.
    Folder,
    Note,
}

/// One node of the scan tree.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
pub struct ObsidianImportNode {
    /// The vault path (`Folder/Sub` or `Folder/Note.md`); use it in `selection.paths`.
    pub path: String,
    pub kind: ObsidianImportNodeKind,
    /// The folder name, or the note's file name without `.md`.
    pub title: String,
    /// The note with the page's content: the note itself, or a folder's folder note
    /// (`Folder/Folder.md`); `null` for a folder without one.
    #[schema(required = true)]
    pub note_path: Option<String>,
    /// The parent folder's `path`; `null` at the top of the vault.
    #[schema(required = true)]
    pub parent_path: Option<String>,
    pub child_count: u32,
}

/// What the archive holds. `nodes` are in tree order: every folder before its children,
/// folders before notes, each by name.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
pub struct ObsidianImportTree {
    pub nodes: Vec<ObsidianImportNode>,
    /// Files (images, PDFs, …) that notes link to or embed; they are attached to those pages.
    pub attachments: u32,
    /// Links and embeds whose target is not in the vault; they stay as text.
    pub unresolved_links: u32,
    pub skipped: ObsidianSkippedFiles,
}

/// Lists and checks the archive, reads every note's links and builds the tree.
pub fn scan<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    limits: VaultLimits,
) -> Result<ObsidianImportTree, VaultError> {
    let mut vault = Vault::open(archive, limits)?;
    let mut referenced: HashSet<String> = HashSet::new();
    let mut unresolved = 0u32;
    for note in &vault.notes {
        let bytes = Vault::read(archive, note)?;
        let markdown = String::from_utf8_lossy(&bytes);
        for (target, _) in obsidian_links(&markdown) {
            match vault.resolve(&note.path, &target) {
                Some(Resolved::Attachment(file)) => {
                    referenced.insert(file.path.clone());
                }
                Some(Resolved::Note(_)) => {}
                None => unresolved = unresolved.saturating_add(1),
            }
        }
    }
    let unreferenced = vault.attachments.len() - referenced.len();
    vault.skipped.unreferenced = u32::try_from(unreferenced).unwrap_or(u32::MAX);
    let mut tree = build_tree(&vault);
    tree.attachments = u32::try_from(referenced.len()).unwrap_or(u32::MAX);
    tree.unresolved_links = unresolved;
    Ok(tree)
}

/// Folders (those that hold notes) and notes as a tree. `Folder/Folder.md` is the folder's
/// note, not a page of its own.
#[must_use]
pub fn build_tree(vault: &Vault) -> ObsidianImportTree {
    let mut folders: BTreeMap<String, Option<String>> = BTreeMap::new();
    for note in &vault.notes {
        let mut path = note.path.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            folders.entry(parent.to_owned()).or_insert(None);
            path = parent;
        }
    }
    let mut folder_notes: HashSet<&str> = HashSet::new();
    for note in &vault.notes {
        let Some((folder, _)) = note.path.rsplit_once('/') else {
            continue;
        };
        let name = folder.rsplit('/').next().unwrap_or(folder);
        if note_title(&note.path).to_lowercase() == name.to_lowercase()
            && let Some(slot) = folders.get_mut(folder)
            && slot.is_none()
        {
            *slot = Some(note.path.clone());
            folder_notes.insert(note.path.as_str());
        }
    }

    let parent_of = |path: &str| path.rsplit_once('/').map(|(parent, _)| parent.to_owned());
    let mut nodes: Vec<ObsidianImportNode> = folders
        .iter()
        .map(|(path, note)| ObsidianImportNode {
            path: path.clone(),
            kind: ObsidianImportNodeKind::Folder,
            title: path.rsplit('/').next().unwrap_or(path).to_owned(),
            note_path: note.clone(),
            parent_path: parent_of(path),
            child_count: 0,
        })
        .collect();
    nodes.extend(
        vault
            .notes
            .iter()
            .filter(|note| !folder_notes.contains(note.path.as_str()))
            .map(|note| ObsidianImportNode {
                path: note.path.clone(),
                kind: ObsidianImportNodeKind::Note,
                title: note_title(&note.path).to_owned(),
                note_path: Some(note.path.clone()),
                parent_path: parent_of(&note.path),
                child_count: 0,
            }),
    );

    let mut children: HashMap<Option<String>, Vec<usize>> = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        children
            .entry(node.parent_path.clone())
            .or_default()
            .push(index);
    }
    for list in children.values_mut() {
        list.sort_by_cached_key(|&index| {
            let node = &nodes[index];
            (
                node.kind != ObsidianImportNodeKind::Folder,
                node.title.to_lowercase(),
                node.path.clone(),
            )
        });
    }
    let mut order = Vec::with_capacity(nodes.len());
    let mut stack: Vec<usize> = children
        .get(&None)
        .map(|roots| roots.iter().rev().copied().collect())
        .unwrap_or_default();
    while let Some(index) = stack.pop() {
        order.push(index);
        if let Some(list) = children.get(&Some(nodes[index].path.clone())) {
            stack.extend(list.iter().rev().copied());
        }
    }
    let ordered = order
        .into_iter()
        .map(|index| {
            let mut node = nodes[index].clone();
            node.child_count = children
                .get(&Some(node.path.clone()))
                .map_or(0, |list| u32::try_from(list.len()).unwrap_or(u32::MAX));
            node
        })
        .collect();
    ObsidianImportTree {
        nodes: ordered,
        attachments: 0,
        unresolved_links: 0,
        skipped: vault.skipped,
    }
}
