// Pure helpers for the page document (a BlockNote block array stored as opaque JSON by the API).
// No BlockNote imports here so they stay cheap to unit-test.

import { normalizeLinkHref, URL_NOISE } from '@orbit/editor/links'

const UUID = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}'

/** The exact download path the server hands out for a page file (`PageFile.url`). */
const PAGE_FILE_PATH = new RegExp(`^/api/v1/workspaces/${UUID}/pages/${UUID}/files/${UUID}$`)

/** True for a page file download path (same origin, so it satisfies the production CSP `img-src 'self'`). */
export function isPageFileUrl(url: unknown): url is string {
  return typeof url === 'string' && PAGE_FILE_PATH.test(url)
}

/** The task whose attachments a task description or comment may show (instead of page files). */
export interface FileScope {
  workspaceId: string
  taskId: string
}

/** The download path of a task attachment, what inline images and files in task text point at. */
export function taskAttachmentUrl(workspaceId: string, taskId: string, attachmentId: string): string {
  return `/api/v1/workspaces/${workspaceId}/tasks/${taskId}/attachments/${attachmentId}/download`
}

const TASK_ATTACHMENT_PATH = new RegExp(`^/api/v1/workspaces/(${UUID})/tasks/(${UUID})/attachments/${UUID}/download$`)

/** True for an attachment download path of a task in the scope's workspace (text copied between tasks keeps its files). */
export function isTaskAttachmentUrl(url: unknown, scope: FileScope): url is string {
  if (typeof url !== 'string') return false
  const match = TASK_ATTACHMENT_PATH.exec(url)
  return match !== null && match[1] === scope.workspaceId
}

/**
 * URL policy for image/file blocks: an uploaded page file (with a `scope`: the task's own attachment), an
 * absolute http(s) URL (embedded by link), or empty (a block still waiting for its upload). Everything else
 * (javascript:, data:, other paths) is dropped.
 */
export function isSafeFileUrl(url: unknown, scope?: FileScope): url is string {
  if (typeof url !== 'string') return false
  const ownFile = scope ? isTaskAttachmentUrl(url, scope) : PAGE_FILE_PATH.test(url)
  if (url === '' || ownFile) return true
  const cleaned = url.replace(URL_NOISE, '')
  return cleaned === url && /^https?:\/\/[^/\\]/i.test(url)
}

const FILE_BLOCKS: ReadonlySet<string> = new Set(['image', 'file'])

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * Deep-walks inline content anywhere in the document (paragraph content, table cells, nested children) and
 * rewrites `{ type: 'link', href, content }` nodes: safe hrefs are normalized, unsafe ones are unwrapped into
 * their plain styled text so no disallowed href survives in stored JSON.
 */
function sanitizeLinks(value: unknown): unknown {
  if (Array.isArray(value)) {
    const out: unknown[] = []
    for (const item of value) {
      if (isRecord(item) && item.type === 'link') {
        const href = normalizeLinkHref(item.href)
        const content = Array.isArray(item.content) ? item.content : []
        if (href) out.push({ ...item, href, content: sanitizeLinks(content) })
        else out.push(...(sanitizeLinks(content) as unknown[]))
        continue
      }
      out.push(sanitizeLinks(item))
    }
    return out
  }
  if (isRecord(value)) {
    const out: Record<string, unknown> = {}
    for (const [key, child] of Object.entries(value)) out[key] = sanitizeLinks(child)
    return out
  }
  return value
}

/** Blanks unsafe `props.url` values of image/file blocks anywhere in the tree (the block stays, empty). */
function sanitizeFileBlocks(blocks: unknown[], scope?: FileScope): unknown[] {
  return blocks.map((block) => {
    if (!isRecord(block)) return block
    let next = block
    if (typeof block.type === 'string' && FILE_BLOCKS.has(block.type) && isRecord(block.props) && !isSafeFileUrl(block.props.url, scope)) {
      next = { ...block, props: { ...block.props, url: '' } }
    }
    return Array.isArray(next.children) ? { ...next, children: sanitizeFileBlocks(next.children, scope) } : next
  })
}

/** Drops blocks whose type is not in the editor schema (BlockNote throws on unknown types), recursively. */
function keepKnownBlocks(blocks: unknown[], allowedTypes: ReadonlySet<string>): Record<string, unknown>[] {
  const out: Record<string, unknown>[] = []
  for (const block of blocks) {
    if (!isRecord(block) || typeof block.type !== 'string' || !allowedTypes.has(block.type)) continue
    const children = Array.isArray(block.children) ? keepKnownBlocks(block.children, allowedTypes) : undefined
    out.push(children ? { ...block, children } : block)
  }
  return out
}

/**
 * Turns API page content (opaque JSON) into safe BlockNote `initialContent`:
 * non-arrays and unknown block types are dropped, links are sanitized, and an empty document becomes
 * `undefined` so BlockNote creates its single empty paragraph (it throws on `[]`).
 */
export function toEditorContent(content: unknown, allowedTypes: ReadonlySet<string>, scope?: FileScope): Record<string, unknown>[] | undefined {
  if (!Array.isArray(content)) return undefined
  const blocks = sanitizeFileBlocks(sanitizeLinks(keepKnownBlocks(content, allowedTypes)) as unknown[], scope) as Record<string, unknown>[]
  return blocks.length > 0 ? blocks : undefined
}

/** Sanitizes the editor document before it is handed to `onChange` (and therefore persisted). */
export function toStoredContent(document: readonly unknown[], scope?: FileScope): unknown[] {
  return sanitizeFileBlocks(sanitizeLinks(document) as unknown[], scope)
}

/**
 * A comment as it is sent: without the empty lines around it. Empty paragraphs at the start and the end go (Enter),
 * and so do line breaks and spaces at the start of the first block and the end of the last (Shift+Enter).
 */
export function trimBlocks<T>(blocks: readonly T[]): T[] {
  let start = 0
  let end = blocks.length
  while (start < end && isBlankParagraph(blocks[start])) start += 1
  while (end > start && isBlankParagraph(blocks[end - 1])) end -= 1
  const kept = blocks.slice(start, end)
  if (kept.length > 0) {
    kept[0] = trimEdgeText(kept[0], 'start')
    kept[kept.length - 1] = trimEdgeText(kept[kept.length - 1], 'end')
  }
  return kept
}

function isWhitespaceText(item: unknown): boolean {
  return isRecord(item) && item.type === 'text' && typeof item.text === 'string' && item.text.trim() === ''
}

function isBlankParagraph(block: unknown): boolean {
  if (!isRecord(block) || block.type !== 'paragraph') return false
  if (Array.isArray(block.children) && block.children.length > 0) return false
  return block.content === undefined || (Array.isArray(block.content) && block.content.every(isWhitespaceText))
}

/** Plain text (not inline code, which keeps its white space). */
function isPlainText(item: unknown): item is Record<string, unknown> & { text: string } {
  return isRecord(item) && item.type === 'text' && typeof item.text === 'string' && !(isRecord(item.styles) && item.styles.code)
}

/**
 * The block with the white space at one end of its text removed, up to inline code (a code block keeps all of it).
 * The server trims comments the same way (`trim_comment_blocks`).
 */
function trimEdgeText<T>(block: T, side: 'start' | 'end'): T {
  if (!isRecord(block) || block.type === 'codeBlock' || !Array.isArray(block.content)) return block
  const content: unknown[] = [...block.content]
  const edge = () => (side === 'start' ? 0 : content.length - 1)
  while (content.length > 0) {
    const item = content[edge()]
    if (!isPlainText(item)) break
    const text = side === 'start' ? item.text.trimStart() : item.text.trimEnd()
    if (text !== '') {
      content[edge()] = { ...item, text }
      break
    }
    content.splice(edge(), 1)
  }
  return { ...block, content }
}

function stableStringify(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(',')}]`
  if (isRecord(value)) {
    const keys = Object.keys(value)
      .filter((key) => value[key] !== undefined)
      .sort()
    return `{${keys.map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(',')}}`
  }
  return JSON.stringify(value) ?? 'null'
}

/**
 * Structural equality that ignores object key order (the server may re-serialize JSON with sorted keys).
 * Used to skip replacing the document when a realtime refresh just echoes our own save.
 */
export function contentEquals(a: unknown, b: unknown): boolean {
  return stableStringify(a) === stableStringify(b)
}

export interface FileToCopy {
  id: string
  url: string
  name: string
}

/**
 * Image/file blocks a paste brought in (not in `before`) that show a file of another task or of a docs page. A task's
 * text keeps its own copy of them, so they stay when the source is trashed (see `PageEditor`'s paste handler).
 */
export function pastedFilesToCopy(document: readonly unknown[], before: ReadonlySet<string>, scope: FileScope): FileToCopy[] {
  const out: FileToCopy[] = []
  const visit = (blocks: readonly unknown[]) => {
    for (const block of blocks) {
      if (!isRecord(block)) continue
      const props = isRecord(block.props) ? block.props : {}
      const url = props.url
      const id = block.id
      if (typeof id === 'string' && !before.has(id) && typeof block.type === 'string' && FILE_BLOCKS.has(block.type) && typeof url === 'string') {
        const task = TASK_ATTACHMENT_PATH.exec(url)
        const foreignTask = task !== null && task[1] === scope.workspaceId && task[2] !== scope.taskId
        if (foreignTask || isPageFileUrl(url)) out.push({ id, url, name: typeof props.name === 'string' && props.name ? props.name : 'file' })
      }
      if (Array.isArray(block.children)) visit(block.children)
    }
  }
  visit(document)
  return out
}


export interface MentionMember {
  id: string
  name: string
}

/** The member whose name starts `text` (longest name first), unless two members share it (as on the server). */
function mentionedMember(text: string, members: readonly MentionMember[]): MentionMember | null {
  const fits = (name: string) => name !== '' && text.startsWith(name) && !/^[\p{L}\p{N}]/u.test(text.slice(name.length))
  const longest = Math.max(0, ...members.filter((member) => fits(member.name)).map((member) => member.name.length))
  if (longest === 0) return null
  const matches = members.filter((member) => member.name.length === longest && fits(member.name))
  return matches.length === 1 ? matches[0] : null
}

/** Splits one plain text node at `@Name` mentions. */
function splitMentions(node: Record<string, unknown>, members: readonly MentionMember[]): unknown[] {
  const text = node.text as string
  const out: unknown[] = []
  let pending = ''
  let at = 0
  while (at < text.length) {
    const index = text.indexOf('@', at)
    if (index === -1) break
    const before = pending + text.slice(at, index)
    const boundary = before === '' || !/[\p{L}\p{N}]$/u.test(before)
    const member = boundary ? mentionedMember(text.slice(index + 1), members) : null
    if (member) {
      if (before) out.push({ ...node, text: before })
      out.push({ type: 'mention', props: { userId: member.id, name: member.name } })
      pending = ''
      at = index + 1 + member.name.length
    } else {
      pending = before + '@'
      at = index + 1
    }
  }
  const rest = pending + text.slice(at)
  if (rest) out.push({ ...node, text: rest })
  return out
}

/**
 * `@Name` in text written before rich text becomes a mention of that member (exact name, one member with it, not in
 * code), as the server's import does, so old comments and descriptions show mention chips.
 */
export function linkMentions(value: unknown, members: readonly MentionMember[]): unknown {
  if (members.length === 0) return value
  if (Array.isArray(value)) {
    return value.flatMap((item) =>
      isRecord(item) && item.type === 'text' && typeof item.text === 'string' && !(isRecord(item.styles) && item.styles.code)
        ? splitMentions(item, members)
        : [linkMentions(item, members)],
    )
  }
  if (isRecord(value)) {
    const out: Record<string, unknown> = {}
    for (const [key, child] of Object.entries(value)) out[key] = linkMentions(child, members)
    return out
  }
  return value
}
