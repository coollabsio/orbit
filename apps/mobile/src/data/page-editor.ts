/**
 * What the native page screen and the page editor (a DOM component, `components/docs/PageEditor.dom.tsx`) say to each
 * other: the toolbar's commands, the editor's formatting state, and the screen's answer to a collab problem. No React.
 */
import type { CollabProblem } from '@orbit/editor/collab/session'

export type InlineStyle = 'bold' | 'italic' | 'underline' | 'strike' | 'code'
export const INLINE_STYLES: readonly InlineStyle[] = ['bold', 'italic', 'underline', 'strike', 'code']

/** The block types the toolbar turns a block into, as BlockNote `type` + the props that tell them apart. */
export const BLOCK_KINDS = {
  paragraph: { label: 'Text', type: 'paragraph' },
  heading1: { label: 'Heading 1', type: 'heading', props: { level: 1 } },
  heading2: { label: 'Heading 2', type: 'heading', props: { level: 2 } },
  heading3: { label: 'Heading 3', type: 'heading', props: { level: 3 } },
  bullet: { label: 'Bulleted list', type: 'bulletListItem' },
  numbered: { label: 'Numbered list', type: 'numberedListItem' },
  check: { label: 'To-do list', type: 'checkListItem' },
  toggle: { label: 'Toggle list', type: 'toggleListItem' },
  quote: { label: 'Quote', type: 'quote' },
  code: { label: 'Code', type: 'codeBlock' },
  callout: { label: 'Callout', type: 'callout' },
} as const satisfies Record<string, { label: string; type: string; props?: Record<string, unknown> }>

export type BlockKind = keyof typeof BLOCK_KINDS

/** The toolbar entry of a block; `null` for blocks it does not offer (images, tables, page cards…). */
export function blockKindOf(block: { type: string; props?: Record<string, unknown> }): BlockKind | null {
  for (const [kind, spec] of Object.entries(BLOCK_KINDS) as [BlockKind, (typeof BLOCK_KINDS)[BlockKind]][]) {
    if (spec.type !== block.type) continue
    const props: Record<string, unknown> = 'props' in spec ? spec.props : {}
    if (Object.entries(props).every(([name, value]) => block.props?.[name] === value)) return kind
  }
  return null
}

export type EditorCommand =
  | { type: 'style'; style: InlineStyle }
  | { type: 'block'; kind: BlockKind }
  | { type: 'indent' }
  | { type: 'outdent' }
  | { type: 'undo' }
  | { type: 'redo' }
  | { type: 'image'; url: string; name: string }
  | { type: 'divider' }
  | { type: 'blur' }

/** A command from the native side (it crosses the bridge as JSON); `null` when it is not one. */
export function parseCommand(value: unknown): EditorCommand | null {
  if (typeof value !== 'object' || value === null) return null
  const command = value as Record<string, unknown>
  switch (command.type) {
    case 'style':
      return INLINE_STYLES.includes(command.style as InlineStyle) ? { type: 'style', style: command.style as InlineStyle } : null
    case 'block':
      return typeof command.kind === 'string' && Object.hasOwn(BLOCK_KINDS, command.kind) ? { type: 'block', kind: command.kind as BlockKind } : null
    case 'image':
      return typeof command.url === 'string' && typeof command.name === 'string' ? { type: 'image', url: command.url, name: command.name } : null
    case 'indent':
    case 'outdent':
    case 'undo':
    case 'redo':
    case 'divider':
    case 'blur':
      return { type: command.type }
    default:
      return null
  }
}

/** The editor's selection, for the toolbar and for keeping the caret above the keyboard. */
export interface FormatState {
  focused: boolean
  styles: Record<InlineStyle, boolean>
  block: BlockKind | null
  canIndent: boolean
  canOutdent: boolean
  /** The caret's line in px from the top of the editor; `null` without a text selection. */
  caret: { top: number; bottom: number } | null
}

export const IDLE_FORMAT: FormatState = {
  focused: false,
  styles: { bold: false, italic: false, underline: false, strike: false, code: false },
  block: null,
  canIndent: false,
  canOutdent: false,
  caret: null,
}

/**
 * What the page screen does when the server closes the editor's socket for good:
 * - `notice`: shown above the page
 * - `refetch`: load the page again (epoch, lock, and access: a 401 there signs out)
 * - `reconnect`: open a new document once the page is loaded again
 * - `readOnly`: stop editing until the page is opened again
 * - `stop`: close the editor (the page is out of reach)
 */
export interface ProblemResponse {
  notice: string | null
  refetch: boolean
  reconnect: boolean
  readOnly: boolean
  stop: boolean
}

const quiet: ProblemResponse = { notice: null, refetch: false, reconnect: false, readOnly: false, stop: false }

export const PROBLEM_RESPONSES: Record<CollabProblem, ProblemResponse> = {
  // the server's document was reset (new epoch) or the page was locked or unlocked
  reset: { ...quiet, refetch: true, reconnect: true },
  'read-only': { ...quiet, notice: 'This workspace is read-only.', refetch: true, reconnect: true, readOnly: true },
  forbidden: { ...quiet, notice: 'You no longer have access to this page.', stop: true },
  gone: { ...quiet, notice: 'This page was moved to the trash or deleted.', refetch: true, stop: true },
  session: { ...quiet, notice: 'Your session ended. Sign in again.', refetch: true, stop: true },
  'too-large': { ...quiet, notice: 'This page is too large to edit here. New changes are not saved.', readOnly: true },
  protocol: { ...quiet, notice: 'This app version cannot edit this page. Update the app.', readOnly: true },
  // the session reconnects by itself
  'rate-limited': quiet,
}

const DOCS_LINK = /^\/docs\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})(?:[?#].*)?$/i

/** The page id of an in-app `/docs/<uuid>` link (or the same on the Orbit server), else `null`. */
export function docsLinkPageId(href: string, server: string): string | null {
  const path = href.startsWith(server.replace(/\/+$/, '') + '/') ? href.slice(server.replace(/\/+$/, '').length) : href
  return DOCS_LINK.exec(path)?.[1]?.toLowerCase() ?? null
}

/**
 * A native action of the DOM editor that never settles once the editor is gone. Expo sends an action's result back
 * with `injectJavaScript` on the editor's web view; after that view unmounted (the page was left while a ticket or a
 * file was still loading) the send rejects with "Unable to find the 'DomWebView' view". Nobody waits for the result
 * any more, so it is dropped instead.
 */
export function whileMounted<Args extends unknown[], Result>(
  mounted: { readonly current: boolean },
  action: (...args: Args) => Promise<Result>,
): (...args: Args) => Promise<Result> {
  const never = new Promise<never>(() => {})
  return (...args) =>
    action(...args).then(
      (result) => (mounted.current ? result : never),
      (error: unknown) => (mounted.current ? Promise.reject(error) : never),
    )
}
