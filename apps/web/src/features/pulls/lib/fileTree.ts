import type { GithubPullFile } from '@orbit/api/generated/types.gen'

export type TreeNode =
  | { kind: 'folder'; name: string; path: string; children: TreeNode[] }
  | { kind: 'file'; name: string; file: GithubPullFile }

type Folder = { folders: Map<string, Folder>; files: GithubPullFile[] }

/** The changed files as a tree: folders first, then files, by name; a folder whose only child is a folder is one row. */
export function buildFileTree(files: GithubPullFile[]): TreeNode[] {
  const root: Folder = { folders: new Map(), files: [] }
  for (const file of files) {
    const parts = file.path.split('/')
    let folder = root
    for (const part of parts.slice(0, -1)) {
      let next = folder.folders.get(part)
      if (!next) { next = { folders: new Map(), files: [] }; folder.folders.set(part, next) }
      folder = next
    }
    folder.files.push(file)
  }
  return nodes(root, '')
}

function nodes(folder: Folder, prefix: string): TreeNode[] {
  const byName = (a: string, b: string) => a.localeCompare(b, undefined, { numeric: true })
  const folders = [...folder.folders.entries()].sort(([a], [b]) => byName(a, b)).map(([name, child]): TreeNode => {
    // merge single-folder chains: apps → web → src becomes "apps/web/src"
    let label = name
    let current = child
    while (current.files.length === 0 && current.folders.size === 1) {
      const [[nextName, next]] = [...current.folders.entries()]
      label = `${label}/${nextName}`
      current = next
    }
    const path = prefix ? `${prefix}/${label}` : label
    return { kind: 'folder', name: label, path, children: nodes(current, path) }
  })
  const files = [...folder.files]
    .sort((a, b) => byName(a.path, b.path))
    .map((file): TreeNode => ({ kind: 'file', name: file.path.slice(file.path.lastIndexOf('/') + 1), file }))
  return [...folders, ...files]
}

/** The files in the order the tree shows them (the diff list uses the same order). */
export function treeOrder(nodes: TreeNode[]): GithubPullFile[] {
  return nodes.flatMap((node) => node.kind === 'file' ? [node.file] : treeOrder(node.children))
}
