/**
 * The workspace imports (Notion and Obsidian into Docs; Asana, Linear and Trello into Tasks) over the generated SDK,
 * with the web's error sentences, report notes, and the pure tree logic of the "Choose pages" step. No React.
 */
import {
  cancelAsanaImport,
  cancelLinearImport,
  cancelNotionImport,
  cancelObsidianImport,
  cancelTrelloImport,
  createAsanaImport,
  createLinearImport,
  createNotionImport,
  createObsidianImport,
  createTrelloImport,
  getAsanaImport,
  getLinearImport,
  getNotionImport,
  getObsidianImport,
  getTrelloImport,
  listAsanaImports,
  listLinearImports,
  listNotionImports,
  listObsidianImports,
  listTrelloImports,
  startAsanaImport,
  startLinearImport,
  startNotionImport,
  startObsidianImport,
  startTrelloImport,
} from '@orbit/api/generated/sdk.gen'
import { notionDatabaseNotes } from '@orbit/database/notionImport'
import type {
  AsanaImport,
  CreateTrelloImportBody,
  LinearImport,
  NotionImport,
  NotionImportDestination,
  NotionImportStatus,
  ObsidianImport,
  ObsidianSkippedFiles,
  StartAsanaImportBody,
  StartLinearImportBody,
  StartNotionImportBody,
  StartObsidianImportBody,
  StartTrelloImportBody,
  TrelloImport,
} from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'

import type { OrbitClient, PageSummary, Teamspace } from './orbit'

export type ImportKind = 'notion' | 'obsidian' | 'asana' | 'linear' | 'trello'

interface Records {
  notion: NotionImport
  obsidian: ObsidianImport
  asana: AsanaImport
  linear: LinearImport
  trello: TrelloImport
}
interface StartBodies {
  notion: StartNotionImportBody
  obsidian: StartObsidianImportBody
  asana: StartAsanaImportBody
  linear: StartLinearImportBody
  trello: StartTrelloImportBody
}

export type ImportRecord<K extends ImportKind> = Records[K]
export type ImportTree<K extends ImportKind> = NonNullable<Records[K]['tree']>
export type StartBody<K extends ImportKind> = StartBodies[K]
/** Every import goes through the same states. */
export type ImportStatus = NotionImportStatus

/** How often an import that is still working is polled (the list: twice as long). */
export const IMPORT_POLL_MS = 2000

export const importKeys = {
  list: (workspaceId: string, kind: ImportKind) => ['imports', workspaceId, kind] as const,
  detail: (workspaceId: string, kind: ImportKind, id: string) => ['imports', workspaceId, kind, id] as const,
  tree: (workspaceId: string, kind: ImportKind, id: string) => ['imports', workspaceId, kind, id, 'tree'] as const,
}

/** Into Docs (pages) or into Tasks (projects). */
export const docsImport = (kind: ImportKind) => kind === 'notion' || kind === 'obsidian'

// The SDK's five families have the same shape; one loose signature serves them all, typed again per kind below.
type Call = (options: { client: OrbitClient; path: Record<string, string>; query?: { include: 'tree' }; body?: unknown; throwOnError: true }) => Promise<{ data: unknown }>
interface Calls {
  list: Call
  get: Call
  start: Call
  cancel: Call
}
const SDK = {
  notion: { list: listNotionImports, get: getNotionImport, start: startNotionImport, cancel: cancelNotionImport },
  obsidian: { list: listObsidianImports, get: getObsidianImport, start: startObsidianImport, cancel: cancelObsidianImport },
  asana: { list: listAsanaImports, get: getAsanaImport, start: startAsanaImport, cancel: cancelAsanaImport },
  linear: { list: listLinearImports, get: getLinearImport, start: startLinearImport, cancel: cancelLinearImport },
  trello: { list: listTrelloImports, get: getTrelloImport, start: startTrelloImport, cancel: cancelTrelloImport },
} as unknown as Record<ImportKind, Calls>

const importPath = (workspaceId: string, importId: string) => ({ workspace_id: workspaceId, import_id: importId })

/** The member's 20 most recent imports of this kind in the workspace (without trees). */
export async function listImports<K extends ImportKind>(client: OrbitClient, kind: K, workspaceId: string): Promise<ImportRecord<K>[]> {
  const { data } = await SDK[kind].list({ client, path: { workspace_id: workspaceId }, throwOnError: true })
  return (data as { items: ImportRecord<K>[] }).items
}

export async function getImport<K extends ImportKind>(client: OrbitClient, kind: K, workspaceId: string, importId: string): Promise<ImportRecord<K>> {
  return (await SDK[kind].get({ client, path: importPath(workspaceId, importId), throwOnError: true })).data as ImportRecord<K>
}

/** The scan tree of a `ready` import (`?include=tree`); it does not change any more. */
export async function getImportTree<K extends ImportKind>(client: OrbitClient, kind: K, workspaceId: string, importId: string): Promise<ImportTree<K> | null> {
  const { data } = await SDK[kind].get({ client, path: importPath(workspaceId, importId), query: { include: 'tree' }, throwOnError: true })
  return (data as ImportRecord<K>).tree as ImportTree<K> | null
}

export async function startImport<K extends ImportKind>(client: OrbitClient, kind: K, workspaceId: string, importId: string, body: StartBody<K>): Promise<ImportRecord<K>> {
  return (await SDK[kind].start({ client, path: importPath(workspaceId, importId), body, throwOnError: true })).data as ImportRecord<K>
}

export async function cancelImport<K extends ImportKind>(client: OrbitClient, kind: K, workspaceId: string, importId: string): Promise<ImportRecord<K>> {
  return (await SDK[kind].cancel({ client, path: importPath(workspaceId, importId), throwOnError: true })).data as ImportRecord<K>
}

// The token is sent once and never kept: these are plain calls, not mutations (whose state keeps the variables).

export async function createNotion(client: OrbitClient, workspaceId: string, token: string): Promise<NotionImport> {
  return (await createNotionImport({ client, path: { workspace_id: workspaceId }, body: { token }, throwOnError: true })).data
}

export async function createAsana(client: OrbitClient, workspaceId: string, token: string): Promise<AsanaImport> {
  return (await createAsanaImport({ client, path: { workspace_id: workspaceId }, body: { token }, throwOnError: true })).data
}

export async function createLinear(client: OrbitClient, workspaceId: string, key: string): Promise<LinearImport> {
  return (await createLinearImport({ client, path: { workspace_id: workspaceId }, body: { token: key }, throwOnError: true })).data
}

export async function createTrello(client: OrbitClient, workspaceId: string, credentials: CreateTrelloImportBody): Promise<TrelloImport> {
  return (await createTrelloImport({ client, path: { workspace_id: workspaceId }, body: credentials, throwOnError: true })).data
}

/** Uploads the vault zip as the `file` part under its own name (a bare Blob has none), and starts the scan. */
export async function uploadObsidian(client: OrbitClient, workspaceId: string, file: Blob, name: string, signal: AbortSignal): Promise<ObsidianImport> {
  const form = () => {
    const body = new FormData()
    body.append('file', file, name)
    return body
  }
  return (await createObsidianImport({ client, path: { workspace_id: workspaceId }, body: { file }, bodySerializer: form, signal, throwOnError: true })).data
}

/** A job is working on it: poll. */
export function isActiveImport(status: ImportStatus | undefined): boolean {
  return status === 'scanning' || status === 'queued' || status === 'importing'
}

/** Nothing will change any more. */
export function isFinalImport(status: ImportStatus | undefined): boolean {
  return status === 'completed' || status === 'failed' || status === 'cancelled' || status === 'expired'
}

export const isImportInProgress = (error: unknown) => error instanceof ApiProblem && error.code === 'import_in_progress'
export const isImportMissing = (error: unknown) => error instanceof ApiProblem && error.status === 404

export const IMPORT_STATUS_LABELS: Record<ImportStatus, string> = {
  scanning: 'Scanning',
  ready: 'Ready to import',
  queued: 'Queued',
  importing: 'Importing',
  completed: 'Completed',
  failed: 'Failed',
  cancelled: 'Cancelled',
  expired: 'Expired',
}

/** The web's badge variant of a status. */
export function importStatusTone(status: ImportStatus): 'primary' | 'secondary' | 'destructive' | 'outline' {
  if (status === 'failed') return 'destructive'
  if (status === 'completed') return 'secondary'
  if (isActiveImport(status) || status === 'ready') return 'primary'
  return 'outline'
}

export function importHeadline(status: ImportStatus): string {
  return status === 'completed' ? 'Import complete' : status === 'failed' ? 'Import failed' : status === 'cancelled' ? 'Import cancelled' : 'Import expired'
}

export const plural = (count: number, one: string, many: string) => `${count} ${count === 1 ? one : many}`

const SOURCE: Record<ImportKind, string> = { notion: 'Notion', obsidian: 'Obsidian', asana: 'Asana', linear: 'Linear', trello: 'Trello' }

/** A user-facing sentence for a failed import request, as each web import page words it. */
export function importErrorMessage(kind: ImportKind, error: unknown): string {
  if (error instanceof ApiProblem) {
    const source = SOURCE[kind]
    switch (error.code) {
      case 'app_key_missing':
        return `Orbit is not ready to import from ${source} yet. Ask the Orbit administrator to set the app key.`
      case 'import_in_progress':
        return 'You already have an import running. Wait for it to finish or cancel it.'
      case `${kind}_unavailable`:
        return `${source} could not be reached. Try again in a moment.`
      case `${kind}_import_state_conflict`:
        return 'This import changed in the meantime (it may have expired or been cancelled).'
      case `${kind}_import_not_found`:
        return 'This import no longer exists.'
      case 'notion_token_invalid':
        return 'Notion rejected this token. Check that you copied the whole token and that it was not revoked.'
      case 'notion_import_too_large':
        return 'An import can contain at most 5,000 pages. Select fewer pages.'
      case 'obsidian_archive_too_large':
        return 'The file is larger than the server’s upload limit. Remove large attachments from the vault and zip it again.'
      case 'obsidian_archive_invalid':
        return 'Upload the vault as one .zip file.'
      case 'obsidian_import_too_large':
        return 'An import can contain at most 5,000 pages. Select fewer notes.'
      case 'asana_token_invalid':
        return 'Asana rejected this token. Check that you copied the whole token and that it was not revoked.'
      case 'linear_token_invalid':
        return 'Linear rejected this API key. Check that you copied the whole key and that it was not revoked.'
      // the detail carries Trello's own reason (invalid key, expired token…)
      case 'trello_key_invalid':
        return error.detail || 'Trello rejected this API key. Copy the key again from the Trello Auth tab of your Power-Up.'
      case 'trello_token_invalid':
        return error.detail || 'Trello rejected this token. Generate a new token for this API key and paste it.'
      case 'page_not_found':
        return 'The destination page no longer exists. Choose another one.'
      case 'teamspace_not_found':
        return 'The destination teamspace no longer exists. Choose another one.'
    }
    if (kind === 'obsidian' && error.status === 413) return 'The file is larger than the server’s upload limit.'
    if (error.status === 422) return 'The request was not valid. Check your input and try again.'
  }
  if (kind === 'obsidian' && error instanceof TypeError) return 'The server could not be reached. Check your connection and try again.'
  return 'Something went wrong. Try again.'
}

/** Which Trello field an error belongs to (shown under it), or null. */
export function trelloCredentialField(error: unknown): 'key' | 'token' | null {
  if (!(error instanceof ApiProblem)) return null
  if (error.code === 'trello_key_invalid') return 'key'
  if (error.code === 'trello_token_invalid') return 'token'
  return null
}

/** Trello's authorization page for a read-only token that expires in a day, for the given API key. */
export const trelloTokenUrl = (key: string) =>
  `https://trello.com/1/authorize?expiration=1day&scope=read&response_type=token&name=Orbit&key=${encodeURIComponent(key)}`

const SKIPPED_LABELS: Record<keyof ObsidianSkippedFiles, [one: string, many: string]> = {
  system: ['settings or system file (.obsidian/, .trash/)', 'settings and system files (.obsidian/, .trash/)'],
  canvas: ['canvas', 'canvases'],
  csv: ['CSV file', 'CSV files'],
  unreferenced: ['file no note links to', 'files no note links to'],
  too_large: ['file over the upload limit', 'files over the upload limit'],
  duplicates: ['file whose name differs only in letter case', 'files whose names differ only in letter case'],
}

/** The skipped files as "2 canvases" parts (zero counts left out) and their total. */
export function skippedSummary(skipped: ObsidianSkippedFiles): { total: number; parts: string[] } {
  const entries = (Object.keys(SKIPPED_LABELS) as (keyof ObsidianSkippedFiles)[]).filter((kind) => skipped[kind] > 0)
  return {
    total: entries.reduce((sum, kind) => sum + skipped[kind], 0),
    parts: entries.map((kind) => `${skipped[kind]} ${SKIPPED_LABELS[kind][skipped[kind] === 1 ? 0 : 1]}`),
  }
}

/** Kinds with a count, most first: "toggle (3), embed (1)". */
export function countList(map: Record<string, number>): string {
  return Object.entries(map)
    .filter(([, count]) => count > 0)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([kind, count]) => `${kind.replaceAll('_', ' ')} (${count})`)
    .join(', ')
}

/** The conversion notes of a finished import, in the web's words. */
export function importNotes<K extends ImportKind>(kind: K, item: ImportRecord<K>): string[] {
  const notes: string[] = []
  const push = (count: number, text: string) => {
    if (count > 0) notes.push(text)
  }
  if (kind === 'notion') {
    const report = (item as NotionImport).report
    if (!report) return notes
    push(report.missing_files, `${report.missing_files} Notion files could not be downloaded.`)
    push(report.unresolved_page_links, `${report.unresolved_page_links} links point to Notion pages that were not imported; they still open Notion.`)
    push(report.truncated_blocks, `${report.truncated_blocks} blocks were cut from pages that were too large.`)
    push(report.previously_imported, `${report.previously_imported} pages were imported before; this import created new copies.`)
    push(report.unfilled_pages_trashed, `${report.unfilled_pages_trashed} empty pages the import had created were moved to the trash.`)
    notes.push(...notionDatabaseNotes(report).notes)
  } else if (kind === 'obsidian') {
    const report = (item as ObsidianImport).report
    if (!report) return notes
    const { missing_files: missing, unresolved_links: unresolved, truncated_blocks: truncated, unfilled_pages_trashed: trashed } = report
    push(missing, missing === 1 ? '1 linked file could not be attached.' : `${missing} linked files could not be attached.`)
    push(unresolved, unresolved === 1 ? '1 link points to a note or file that was not imported; it stayed as text.' : `${unresolved} links point to notes or files that were not imported; they stayed as text.`)
    push(truncated, truncated === 1 ? '1 block was cut from a note that was too large.' : `${truncated} blocks were cut from notes that were too large.`)
    push(trashed, trashed === 1 ? '1 empty page the import had created was moved to the trash.' : `${trashed} empty pages the import had created were moved to the trash.`)
  } else if (kind === 'asana') {
    const report = (item as AsanaImport).report
    if (!report) return notes
    push(report.dependencies_linked, `${report.dependencies_linked} dependencies became “blocked by” relations.`)
    push(report.links_added, `${report.links_added} linked files (Google Drive, Dropbox and others) were added to descriptions as links.`)
    push(report.unmatched_assignees, `${report.unmatched_assignees} tasks had an assignee with no Orbit member of the same email; they are unassigned and name the assignee.`)
    push(report.missing_files, `${report.missing_files} Asana files could not be downloaded.`)
    push(report.skipped_subtasks, `${report.skipped_subtasks} subtasks deeper than five levels were not imported.`)
  } else if (kind === 'linear') {
    const report = (item as LinearImport).report
    if (!report) return notes
    push(report.relations_linked, `${report.relations_linked} blocking, related and duplicate relations were linked.`)
    push(report.relations_skipped, `${report.relations_skipped} relations were skipped: Orbit keeps one relation per pair of tasks and one duplicate per task.`)
    push(report.links_added, `${report.links_added} linked attachments (pull requests, threads and links) were added to descriptions.`)
    push(report.unmatched_assignees, `${report.unmatched_assignees} issues had an assignee with no Orbit member of the same email; they are unassigned and name the assignee.`)
    push(report.missing_files, `${report.missing_files} Linear files could not be downloaded.`)
    push(report.detached_sub_issues, `${report.detached_sub_issues} sub-issues are at the top level (parent in another team, or more than five levels deep); their description names the parent.`)
  } else {
    const report = (item as TrelloImport).report
    if (!report) return notes
    push(report.checklists_added, `${plural(report.checklists_added, 'checklist was', 'checklists were')} written into card descriptions.`)
    push(report.links_added, `${plural(report.links_added, 'link attachment was', 'link attachments were')} added to card descriptions.`)
    push(report.unmatched_members, `${plural(report.unmatched_members, 'card member has', 'card members have')} no Orbit member of the same full name; the card description names them.`)
    push(report.archived_lists_skipped, `${plural(report.archived_lists_skipped, 'archived list and its', 'archived lists and their')} cards were left out.`)
    push(report.archived_cards_skipped, `${plural(report.archived_cards_skipped, 'archived card was', 'archived cards were')} left out.`)
    push(report.refused_files, `Trello refused ${plural(report.refused_files, 'file download', 'file downloads')}; some Enterprise workspaces block them.`)
    push(report.missing_files, `${plural(report.missing_files, 'Trello file', 'Trello files')} could not be downloaded or attached (too large, or gone).`)
  }
  return notes
}

// Docs destination: a space (a teamspace or Private), optionally inside one of its pages.

export type SpaceKey = 'private' | `teamspace:${string}`
export const PRIVATE_SPACE: SpaceKey = 'private'
/** The "Inside page" value for the space's top level. */
export const TOP_LEVEL = '__top__'

export const teamspaceSpace = (teamspaceId: string): SpaceKey => `teamspace:${teamspaceId}`
const spaceOf = (page: PageSummary): SpaceKey => (page.private || !page.teamspace_id ? PRIVATE_SPACE : teamspaceSpace(page.teamspace_id))
const byPosition = (a: PageSummary, b: PageSummary) => a.position - b.position || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)

/** The `destination` body of a Docs import's start. */
export type DestinationBody = { parent_page_id: string } | { teamspace_id: string } | { private: true }

export function spaceBody(space: SpaceKey): { teamspace_id: string } | { private: true } {
  return space === PRIVATE_SPACE ? { private: true } : { teamspace_id: space.slice('teamspace:'.length) }
}

/** Where new root pages go by default: the teamspace flagged `is_default`, else the first by position. */
export function defaultTeamspace(teamspaces: readonly Teamspace[] | undefined): Teamspace | null {
  if (!teamspaces?.length) return null
  return teamspaces.find((teamspace) => teamspace.is_default) ?? [...teamspaces].sort((a, b) => a.position - b.position)[0]
}

/** Pages of one space in tree order, with their depth (the "Inside page" options). */
export function spacePages(pages: readonly PageSummary[], space: SpaceKey): { page: PageSummary; depth: number }[] {
  const out: { page: PageSummary; depth: number }[] = []
  const walk = (list: PageSummary[], depth: number) => {
    for (const page of list) {
      out.push({ page, depth })
      walk(pages.filter((child) => child.parent_id === page.id).sort(byPosition), depth + 1)
    }
  }
  walk(pages.filter((page) => page.parent_id === null && spaceOf(page) === space).sort(byPosition), 0)
  return out
}

/** Where an import's pages went, for the summary ("General", "Private", or "Inside “Roadmap”"). */
export function destinationLabel(destination: NotionImportDestination | null, teamspaces: readonly Teamspace[] | undefined, pages: readonly PageSummary[] | undefined): string | null {
  if (!destination) return null
  if (destination.parent_page_id) {
    const page = pages?.find((item) => item.id === destination.parent_page_id)
    return page ? `Inside “${page.title.trim() || 'Untitled'}”` : 'Inside a page'
  }
  if (destination.private) return 'Private'
  return teamspaces?.find((teamspace) => teamspace.id === destination.teamspace_id)?.name ?? 'Teamspace'
}

// The tri-state tree of the Docs imports' "Choose pages" step (the web's `docs/import/selection.ts`). A checked node
// always means "this node and everything under it", so the selected set stays closed under descendants.

export interface TreeNode {
  title: string
}

export interface SelectionTree<T extends TreeNode = TreeNode> {
  /** Node ids in tree order (parents before children). */
  ids: string[]
  byId: Map<string, T>
  /** Child ids per parent id; `null` holds the roots (no parent, or a parent missing from the list). */
  children: Map<string | null, string[]>
  parents: Map<string, string | null>
}

export type CheckState = 'checked' | 'unchecked' | 'indeterminate'

export function buildSelectionTree<T extends TreeNode>(nodes: readonly T[], identify: (node: T) => readonly [id: string, parentId: string | null]): SelectionTree<T> {
  const byId = new Map<string, T>()
  const declared = new Map<string, string | null>()
  for (const node of nodes) {
    const [id, parentId] = identify(node)
    if (byId.has(id)) continue
    byId.set(id, node)
    declared.set(id, parentId)
  }
  const children = new Map<string | null, string[]>()
  const parents = new Map<string, string | null>()
  for (const [id, parentId] of declared) {
    const parent = parentId !== null && parentId !== id && byId.has(parentId) ? parentId : null
    parents.set(id, parent)
    const list = children.get(parent)
    if (list) list.push(id)
    else children.set(parent, [id])
  }
  return { ids: [...byId.keys()], byId, children, parents }
}

const childIds = (tree: SelectionTree, id: string | null) => tree.children.get(id) ?? []

function subtreeIds(tree: SelectionTree, id: string): string[] {
  const out: string[] = []
  const seen = new Set<string>()
  const stack = [id]
  while (stack.length > 0) {
    const current = stack.pop()!
    if (seen.has(current) || !tree.byId.has(current)) continue
    seen.add(current)
    out.push(current)
    const kids = childIds(tree, current)
    for (let index = kids.length - 1; index >= 0; index -= 1) stack.push(kids[index])
  }
  return out
}

function ancestorIds(tree: SelectionTree, id: string): string[] {
  const out: string[] = []
  const seen = new Set<string>([id])
  let parent = tree.parents.get(id) ?? null
  while (parent !== null && !seen.has(parent)) {
    seen.add(parent)
    out.push(parent)
    parent = tree.parents.get(parent) ?? null
  }
  return out
}

export const selectAll = (tree: SelectionTree) => new Set(tree.byId.keys())

/** Checks `id` with its subtree, or (when it is already checked) unchecks its subtree and its ancestors. */
export function toggleNode(tree: SelectionTree, selected: ReadonlySet<string>, id: string): Set<string> {
  const next = new Set(selected)
  if (!tree.byId.has(id)) return next
  if (selected.has(id)) {
    for (const item of subtreeIds(tree, id)) next.delete(item)
    for (const item of ancestorIds(tree, id)) next.delete(item)
  } else {
    for (const item of subtreeIds(tree, id)) next.add(item)
  }
  return next
}

/** Checked, indeterminate (something below is selected) or unchecked, for every node in one bottom-up pass. */
export function checkStates(tree: SelectionTree, selected: ReadonlySet<string>): Map<string, CheckState> {
  const states = new Map<string, CheckState>()
  const anyBelow = new Map<string, boolean>()
  for (let index = tree.ids.length - 1; index >= 0; index -= 1) {
    const id = tree.ids[index]
    const below = childIds(tree, id).some((child) => selected.has(child) || anyBelow.get(child) === true)
    anyBelow.set(id, below)
    states.set(id, selected.has(id) ? 'checked' : below ? 'indeterminate' : 'unchecked')
  }
  return states
}

export function selectedCount(tree: SelectionTree, selected: ReadonlySet<string>): number {
  let count = 0
  for (const id of tree.byId.keys()) if (selected.has(id)) count += 1
  return count
}

/** What `start` imports: `{ all: true }` when every node is checked, else the minimal roots; null when empty. */
export function importSelection(tree: SelectionTree, selected: ReadonlySet<string>): { all: true } | { roots: string[] } | null {
  const count = selectedCount(tree, selected)
  if (count === 0) return null
  if (count === tree.byId.size) return { all: true }
  return {
    roots: tree.ids.filter((id) => {
      if (!selected.has(id)) return false
      const parent = tree.parents.get(id) ?? null
      return parent === null || !selected.has(parent)
    }),
  }
}

export interface VisibleRow<T extends TreeNode = TreeNode> {
  id: string
  node: T
  depth: number
  hasChildren: boolean
  expanded: boolean
  /** The row's title matches the filter. */
  match: boolean
}

export const nodeTitle = (node: TreeNode) => (node.title.trim() ? node.title : 'Untitled')

/** Without a query: roots plus the children of expanded nodes. With one: the matches and their ancestors, opened. */
export function visibleRows<T extends TreeNode>(tree: SelectionTree<T>, expanded: ReadonlySet<string>, query = ''): VisibleRow<T>[] {
  const needle = query.trim().toLowerCase()
  const rows: VisibleRow<T>[] = []
  if (!needle) {
    const walk = (parent: string | null, depth: number, seen: Set<string>) => {
      for (const id of childIds(tree, parent)) {
        if (seen.has(id)) continue
        seen.add(id)
        const hasChildren = childIds(tree, id).length > 0
        const open = hasChildren && expanded.has(id)
        rows.push({ id, node: tree.byId.get(id)!, depth, hasChildren, expanded: open, match: false })
        if (open) walk(id, depth + 1, seen)
      }
    }
    walk(null, 0, new Set())
    return rows
  }
  const matches = new Set<string>()
  const shown = new Set<string>()
  for (const [id, node] of tree.byId) {
    if (!nodeTitle(node).toLowerCase().includes(needle)) continue
    matches.add(id)
    shown.add(id)
    for (const ancestor of ancestorIds(tree, id)) shown.add(ancestor)
  }
  const walk = (parent: string | null, depth: number) => {
    for (const id of childIds(tree, parent)) {
      if (!shown.has(id)) continue
      shown.delete(id)
      const kids = childIds(tree, id)
      rows.push({ id, node: tree.byId.get(id)!, depth, hasChildren: kids.length > 0, expanded: kids.some((child) => shown.has(child)), match: matches.has(id) })
      walk(id, depth + 1)
    }
  }
  walk(null, 0)
  return rows
}
