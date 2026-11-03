// File blocks whose stored file an admin deleted (Settings → General → Storage). The server keeps the link and
// answers it with 410 and a "Deleted file" image, which an image block shows as it is; a file block only shows its
// name, so it gets a "Deleted file" line here. The lines come from CSS rules on the block's `data-url` in an adopted
// style sheet (the production CSP blocks `<style>` tags), so the editor's own DOM stays untouched.

interface BlockLike {
  type: string
  props?: Record<string, unknown>
  children?: readonly BlockLike[]
}

interface EditorLike {
  readonly document: readonly BlockLike[]
  onChange: (callback: () => void) => (() => void) | void
}

/** The Orbit file URLs (`/api/…`) of the file blocks of a document, nested blocks included. */
export function fileBlockUrls(blocks: readonly BlockLike[]): string[] {
  const urls = new Set<string>()
  const visit = (list: readonly BlockLike[]) => {
    for (const block of list) {
      const url = block.props?.url
      if (block.type === 'file' && typeof url === 'string' && url.startsWith('/api/')) urls.add(url)
      if (block.children) visit(block.children)
    }
  }
  visit(blocks)
  return [...urls]
}

/** The rules that mark the file blocks of one deleted URL. */
export function deletedFileRules(url: string): string[] {
  const block = `.bn-block-content[data-content-type="file"][data-url=${JSON.stringify(url)}]`
  return [
    `${block} .bn-file-name { text-decoration: line-through; opacity: 0.6; }`,
    `${block} .bn-file-block-content-wrapper::after { content: "Deleted file · an admin deleted it"; display: block; margin-top: 2px; font-size: 12px; opacity: 0.7; }`,
  ]
}

const checked = new Map<string, Promise<boolean>>()
let sheet: CSSStyleSheet | null = null

function markDeleted(url: string) {
  if (typeof CSSStyleSheet === 'undefined' || !('adoptedStyleSheets' in document)) return
  if (!sheet) {
    sheet = new CSSStyleSheet()
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet]
  }
  for (const rule of deletedFileRules(url)) sheet.insertRule(rule, sheet.cssRules.length)
}

/**
 * Checks each Orbit file of the editor's file blocks once per session (`isDeleted`, for example a HEAD request that
 * answers 410) and marks the deleted ones. Returns the unsubscribe.
 */
export function watchDeletedFiles(editor: EditorLike, isDeleted: (url: string) => Promise<boolean>): () => void {
  const scan = () => {
    for (const url of fileBlockUrls(editor.document)) {
      if (checked.has(url)) continue
      const check = isDeleted(url).catch(() => false)
      checked.set(url, check)
      void check.then((deleted) => deleted && markDeleted(url))
    }
  }
  scan()
  return editor.onChange(scan) ?? (() => {})
}
