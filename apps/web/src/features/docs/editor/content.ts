// Pure helpers for the page document (a BlockNote block array stored as opaque JSON by the API).
// No BlockNote imports here so they stay cheap to unit-test.

/** Control characters and whitespace browsers strip from URLs before parsing the scheme. */
// eslint-disable-next-line no-control-regex
const URL_NOISE = /[\u0000-\u0020\u007f-\u009f\u00a0\u1680\u180e\u2000-\u2029\u205f\u3000\ufeff]/g

const SAFE_SCHEME = /^(?:https?:\/\/|mailto:)/i
const ANY_SCHEME = /^[a-z][a-z0-9+.-]*:/i
const UUID = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}'
/** An in-app page link, exactly `/docs/<page uuid>` (what the Notion import writes for links between pages). */
const INTERNAL_PAGE_LINK = new RegExp(`^/docs/(${UUID})$`, 'i')

/** The page id of an internal page link (`/docs/<uuid>`), or null for anything else. */
export function internalPageLinkId(href: unknown): string | null {
  if (typeof href !== 'string') return null
  const match = INTERNAL_PAGE_LINK.exec(href)
  return match ? match[1].toLowerCase() : null
}

/** Link policy for the editor: absolute http(s) and mailto hrefs, and exact in-app page links `/docs/<uuid>`. */
export function isSafeLinkHref(href: unknown): href is string {
  if (typeof href !== 'string') return false
  return internalPageLinkId(href) !== null || SAFE_SCHEME.test(href.replace(URL_NOISE, ''))
}

/**
 * Normalizes a stored/pasted href: keeps http(s)/mailto and `/docs/<uuid>` page links, upgrades bare hosts
 * ("example.com/x") to https, and returns null for everything else (javascript:, data:, other relative paths,
 * fragments, other schemes).
 */
export function normalizeLinkHref(href: unknown): string | null {
  if (typeof href !== 'string') return null
  const cleaned = href.replace(URL_NOISE, '')
  if (!cleaned) return null
  if (SAFE_SCHEME.test(cleaned)) return cleaned
  if (cleaned === href && internalPageLinkId(cleaned)) return cleaned
  // Anything that looks like a scheme is rejected, except a dotted host with a port ("example.com:8080/x").
  if (ANY_SCHEME.test(cleaned) && !/^[^:/]*\.[^:/]*:\d+(?:[/?#]|$)/.test(cleaned)) return null
  if (/^[/?#.\\]/.test(cleaned)) return null
  // Bare host: needs a dot in the host part (same rule BlockNote uses for autolinking).
  const host = cleaned.split(/[/?#]/)[0]
  if (!host.includes('.')) return null
  return `https://${cleaned}`
}

/** The exact download path the server hands out for a page file (`PageFile.url`). */
const PAGE_FILE_PATH = new RegExp(`^/api/v1/workspaces/${UUID}/pages/${UUID}/files/${UUID}$`)

/** True for a page file download path (same origin, so it satisfies the production CSP `img-src 'self'`). */
export function isPageFileUrl(url: unknown): url is string {
  return typeof url === 'string' && PAGE_FILE_PATH.test(url)
}

/**
 * URL policy for image/file blocks: an uploaded page file, an absolute http(s) URL (embedded by link), or empty
 * (a block still waiting for its upload). Everything else (javascript:, data:, other paths) is dropped.
 */
export function isSafeFileUrl(url: unknown): url is string {
  if (typeof url !== 'string') return false
  if (url === '' || PAGE_FILE_PATH.test(url)) return true
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
function sanitizeFileBlocks(blocks: unknown[]): unknown[] {
  return blocks.map((block) => {
    if (!isRecord(block)) return block
    let next = block
    if (typeof block.type === 'string' && FILE_BLOCKS.has(block.type) && isRecord(block.props) && !isSafeFileUrl(block.props.url)) {
      next = { ...block, props: { ...block.props, url: '' } }
    }
    return Array.isArray(next.children) ? { ...next, children: sanitizeFileBlocks(next.children) } : next
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
export function toEditorContent(content: unknown, allowedTypes: ReadonlySet<string>): Record<string, unknown>[] | undefined {
  if (!Array.isArray(content)) return undefined
  const blocks = sanitizeFileBlocks(sanitizeLinks(keepKnownBlocks(content, allowedTypes)) as unknown[]) as Record<string, unknown>[]
  return blocks.length > 0 ? blocks : undefined
}

/** Sanitizes the editor document before it is handed to `onChange` (and therefore persisted). */
export function toStoredContent(document: readonly unknown[]): unknown[] {
  return sanitizeFileBlocks(sanitizeLinks(document) as unknown[])
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
