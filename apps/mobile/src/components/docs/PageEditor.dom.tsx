'use dom'

// The page body editor: BlockNote with Orbit's schema (`@orbit/editor`), bound to the page's Y.Doc over the collab
// socket, so it co-edits the same document as the web. It runs in a web view; the screen around it is native and
// talks to it through props, native actions (async functions) and `run` (the toolbar's commands).
import '@blocknote/react/style.css'
import 'katex/dist/katex.min.css'
import './PageEditor.css'

import { withCollaboration } from '@blocknote/core/yjs'
import { BlockNoteViewRaw, useCreateBlockNote } from '@blocknote/react'
import { setEditableInPlace } from '@orbit/editor/editable'
import { commentMarks } from '@orbit/editor/commentMarks'
import { CollabSession, type CollabProblem, type CollabState } from '@orbit/editor/collab/session'
import { collabSocket, COLLAB_FRAGMENT } from '@orbit/editor/collab/target'
import { ticketWebSocket, type TicketResult } from '@orbit/editor/collab/ticketSocket'
import { watchDeletedFiles } from '@orbit/editor/deletedFiles'
import { isSafeLinkHref } from '@orbit/editor/links'
import { selectBlockExtension } from '@orbit/editor/selectBlock'
import { collabUndoExtension } from '@orbit/editor/collab/undo'
import { CALLOUT_DEFAULT_EMOJI, createEditorSchemas, type PageEditorInstance } from '@orbit/editor/schema'
import { type DOMImperativeFactory, useDOMImperativeHandle } from 'expo/dom'
import katex from 'katex'
import { createContext, type CSSProperties, type Ref, use, useEffect, useRef, useState } from 'react'
import { WebsocketProvider } from 'y-websocket'
import * as Y from 'yjs'

import { type DatabaseBlockRef, databaseBlocks, databaseCardKey, type DatabaseCardInfo } from '../../data/database-card'
import { BLOCK_KINDS, blockKindOf, docsLinkPageId, type EditorCommand, type FormatState, INLINE_STYLES, parseCommand } from '../../data/page-editor'

/** What a page card shows about the page it links to. */
export interface PageCardInfo {
  title: string
  icon: string | null
  trashed: boolean
}

export interface PageEditorColors {
  background: string
  foreground: string
  mutedForeground: string
  muted: string
  border: string
  primary: string
}

/** What a ref method receives over the bridge: JSON. */
type BridgeValue = Parameters<DOMImperativeFactory[string]>[number]

export interface PageEditorRef extends DOMImperativeFactory {
  /** Runs an `EditorCommand` (it crosses the bridge as JSON). */
  run: (command: BridgeValue) => void
}

export interface PageEditorProps {
  /** The Orbit server, `https://…` (the socket opens on the same host). */
  server: string
  workspaceId: string
  pageId: string
  /** `Page.collab_epoch`. */
  epoch: string
  /** Bump to drop the document and connect again (after a reset). */
  generation: number
  editable: boolean
  theme: 'light' | 'dark'
  colors: PageEditorColors
  /** The local caret's name and color for the others. */
  user: { name: string; color: string }
  /** Page cards' targets, by page id. */
  pages: Record<string, PageCardInfo>
  /** Database cards, by `databaseCardKey`: the screen fetches the blocks `onDatabases` reports. */
  databases: Record<string, DatabaseCardInfo>
  /** Member names for @mention chips, by user id. */
  members: Record<string, string>
  getTicket: () => Promise<TicketResult>
  /** The data URL of an Orbit file (`/api/…`, which needs the session); `null` when it cannot be loaded. */
  loadFile: (url: string) => Promise<string | null>
  /** An Orbit file an admin deleted (it answers 410): its file block says "Deleted file". */
  fileDeleted: (url: string) => Promise<boolean>
  openPage: (pageId: string) => Promise<void>
  /** The database screen of a card, on the view it shows. */
  openDatabase: (block: DatabaseBlockRef, viewId: string | null) => Promise<void>
  /** The distinct database blocks of the document, whenever they change. */
  onDatabases: (blocks: DatabaseBlockRef[]) => Promise<void>
  openUrl: (url: string) => Promise<void>
  onProblem: (problem: CollabProblem) => Promise<void>
  /** Handshakes keep failing: the screen checks the page over REST. */
  onProbe: () => Promise<void>
  onFormat: (state: FormatState) => Promise<void>
  /** The editor's height in px, for the native view that holds it (the page scrolls natively). */
  onHeight: (height: number) => Promise<void>
  ref?: Ref<PageEditorRef>
  dom?: import('expo/dom').DOMProps
}

interface Lookups {
  pages: Record<string, PageCardInfo>
  databases: Record<string, DatabaseCardInfo>
  members: Record<string, string>
  openPage: (pageId: string) => void
  openDatabase: (block: DatabaseBlockRef, viewId: string | null) => void
}

// Node views render through portals inside the editor view, so they see this provider.
const LookupsContext = createContext<Lookups>({ pages: {}, databases: {}, members: {}, openPage: () => {}, openDatabase: () => {} })

function PageCard({ block }: { block: { props: { pageId: string } } }) {
  const { pages, openPage } = use(LookupsContext)
  const page = pages[block.props.pageId]
  return (
    <button
      type="button"
      contentEditable={false}
      className="orbit-page-card"
      data-state={!page ? 'missing' : page.trashed ? 'trashed' : 'ok'}
      onMouseDown={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.preventDefault()
        if (page && !page.trashed) openPage(block.props.pageId)
      }}
    >
      <span className="orbit-page-card-icon">{page?.icon ?? '📄'}</span>
      <span className="orbit-page-card-title">{page ? page.title.trim() || 'Untitled' : 'Missing page'}</span>
      {page?.trashed ? <span className="orbit-page-card-note">Page in trash</span> : null}
    </button>
  )
}

const DATABASE_NOTES: Record<Exclude<DatabaseCardInfo['state'], 'ready'>, string> = {
  loading: 'Loading…',
  trashed: 'Database in trash',
  missing: 'Database not found',
  'no-access': 'No access to this database',
  'link-gone': 'View not found',
  failed: 'Could not load the database',
}

/**
 * A database shown in the page (spec 3.7): its icon, title, row count and first view, as a card that opens the
 * database screen. The table itself is only on the database screen. A linked view has a "↗" after its title.
 */
function DatabaseCard({ block }: { block: { props: { databaseId: string; linkId: string } } }) {
  const { databases, openDatabase } = use(LookupsContext)
  const ref = { databaseId: block.props.databaseId, linkId: block.props.linkId }
  const card = databases[databaseCardKey(ref)] ?? { state: 'loading' }
  const named = card.state === 'ready' || card.state === 'trashed'
  return (
    <button
      type="button"
      contentEditable={false}
      className="orbit-page-card orbit-database-card"
      data-state={card.state}
      onMouseDown={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.preventDefault()
        if (card.state === 'ready') openDatabase(ref, card.view?.id ?? null)
      }}
    >
      <span className="orbit-page-card-icon">{named && card.icon ? card.icon : <DatabaseIcon />}</span>
      {named ? <span className="orbit-page-card-title">{card.title.trim() || 'Untitled'}</span> : null}
      {named && ref.linkId ? <span className="orbit-database-card-linked" aria-label="Linked view">↗</span> : null}
      <span className="orbit-page-card-note">{card.state === 'ready' ? readyNote(card) : DATABASE_NOTES[card.state]}</span>
    </button>
  )
}

function readyNote(card: Extract<DatabaseCardInfo, { state: 'ready' }>): string {
  const rows = card.rowCount === 1 ? '1 row' : `${card.rowCount.toLocaleString('en-US')} rows`
  return card.view ? `${rows} · ${card.view.name}` : rows
}

/** The tree's database icon (a grid), drawn in the text color. */
function DatabaseIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <path d="M3 9h18M3 15h18M9 3v18" />
    </svg>
  )
}

function Callout({ block, contentRef }: { block: { props: { emoji: string } }; contentRef: (node: HTMLElement | null) => void }) {
  return (
    <div className="orbit-callout">
      <span className="orbit-callout-emoji" contentEditable={false}>
        {block.props.emoji || CALLOUT_DEFAULT_EMOJI}
      </span>
      <div ref={contentRef} className="orbit-callout-text" />
    </div>
  )
}

function InlineMath({ inlineContent }: { inlineContent: { props: { latex: string } } }) {
  const latex = inlineContent.props.latex.trim()
  if (!latex.trim()) return <span className="orbit-math">formula</span>
  try {
    const html = katex.renderToString(latex, { displayMode: false, throwOnError: true, trust: false, strict: 'ignore' })
    return <span className="orbit-math" dangerouslySetInnerHTML={{ __html: html }} />
  } catch {
    return <span className="orbit-math">{latex}</span>
  }
}

function Mention({ inlineContent }: { inlineContent: { props: { userId: string; name: string } } }) {
  const { members } = use(LookupsContext)
  const name = members[inlineContent.props.userId] ?? (inlineContent.props.name || 'Unknown user')
  return (
    <span className="orbit-mention" contentEditable={false}>
      @{name}
    </span>
  )
}

const COLORED_TOKENS = new Set(['keyword', 'string', 'comment', 'class', 'property', 'entity', 'jsxliterals'])

const { pageEditorSchema } = createEditorSchemas({
  page: { render: PageCard },
  database: { render: DatabaseCard },
  callout: { render: Callout },
  mention: { render: Mention },
  math: { render: InlineMath },
  codeBlock: { tokenClass: (type) => (COLORED_TOKENS.has(type) ? `orbit-token-${type}` : undefined) },
})

interface Connection {
  doc: Y.Doc
  provider: WebsocketProvider
  fragment: Y.XmlFragment
}

const CONNECTING: CollabState = { status: 'connecting', problem: null, ready: false }

export default function PageEditor(props: PageEditorProps) {
  const { server, workspaceId, pageId, epoch, generation, editable, theme, colors, pages, databases, members } = props
  // Native actions arrive as new functions with every prop update; the socket and the editor read the latest.
  const actions = useRef(props)
  useEffect(() => {
    actions.current = props
  })
  const editorRef = useRef<PageEditorInstance | null>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const [session, setSession] = useState<{ connection: Connection; state: CollabState } | null>(null)

  useDOMImperativeHandle(
    props.ref as Ref<PageEditorRef>,
    () => ({
      run: (value: BridgeValue) => {
        const command = parseCommand(value)
        if (command && editorRef.current) runCommand(editorRef.current, command)
      },
    }),
    [],
  )

  useEffect(() => {
    const doc = new Y.Doc()
    const { serverUrl, room, params } = collabSocket(server, { workspaceId, kind: 'pages', id: pageId, epoch })
    const provider = new WebsocketProvider(serverUrl, room, doc, {
      params,
      disableBc: true,
      maxBackoffTime: 5000,
      WebSocketPolyfill: ticketWebSocket(() => actions.current.getTicket()),
    })
    provider.awareness.setLocalStateField('user', actions.current.user)
    const connection = { doc, provider, fragment: doc.getXmlFragment(COLLAB_FRAGMENT) }
    const current = new CollabSession(connection, {
      onChange: (state) => setSession((shown) => (shown?.connection === connection ? { connection, state } : shown)),
      onProblem: (problem) => void actions.current.onProblem(problem),
      onProbe: () => void actions.current.onProbe(),
    })
    setSession({ connection, state: current.state })
    return () => {
      current.destroy()
      setSession(null)
    }
  }, [server, workspaceId, pageId, epoch, generation])

  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    let last = 0
    const observer = new ResizeObserver(() => {
      const height = Math.ceil(root.getBoundingClientRect().height)
      if (height === last) return
      last = height
      void actions.current.onHeight(height)
    })
    observer.observe(root)
    return () => observer.disconnect()
  }, [])

  const style = {
    '--orbit-background': colors.background,
    '--orbit-foreground': colors.foreground,
    '--orbit-muted-foreground': colors.mutedForeground,
    '--orbit-muted': colors.muted,
    '--orbit-border': colors.border,
    '--orbit-primary': colors.primary,
  } as CSSProperties

  const state = session?.state ?? CONNECTING
  return (
    <div ref={rootRef} className="orbit-page-editor" data-theme={theme} style={style}>
      {session && state.ready ? (
        <LookupsContext
          value={{
            pages,
            databases,
            members,
            openPage: (id) => void actions.current.openPage(id),
            openDatabase: (block, viewId) => void actions.current.openDatabase(block, viewId),
          }}
        >
          <Editor
            key={`${pageId}:${epoch}:${generation}`}
            connection={session.connection}
            editable={editable && state.problem !== 'read-only'}
            theme={theme}
            server={server}
            actions={actions}
            editorRef={editorRef}
          />
        </LookupsContext>
      ) : (
        <div className="orbit-page-editor-loading" aria-label="Loading the page">
          <span />
          <span />
          <span />
        </div>
      )}
    </div>
  )
}

interface EditorProps {
  connection: Connection
  editable: boolean
  theme: 'light' | 'dark'
  server: string
  actions: { current: PageEditorProps }
  editorRef: { current: PageEditorInstance | null }
}

function Editor({ connection, editable, theme, server, actions, editorRef }: EditorProps) {
  const [files] = useState(() => new Map<string, Promise<string>>())
  const rootRef = useRef<HTMLDivElement>(null)
  const editor: PageEditorInstance = useCreateBlockNote(
    // The server initializes every document, so there is no initial content: the fragment is the page.
    withCollaboration({
      schema: pageEditorSchema,
      // Without the comment mark, y-prosemirror would drop the text of anchored comments for everyone.
      extensions: [commentMarks(), selectBlockExtension(), collabUndoExtension()],
      collaboration: {
        provider: connection.provider,
        fragment: connection.fragment,
        user: actions.current.user,
        showCursorLabels: 'activity',
      },
      // the web's link policy: http(s), mailto and in-app page links; any other href is neither created nor opened
      links: {
        isValidLink: isSafeLinkHref,
        onClick: (event: MouseEvent) => {
          const href = event.target instanceof Element ? event.target.closest('a')?.getAttribute('href') : null
          if (!href) return true
          const pageId = docsLinkPageId(href, server)
          if (pageId) void actions.current.openPage(pageId)
          else if (isSafeLinkHref(href)) void actions.current.openUrl(href)
          return true
        },
      },
      // Orbit files need the session, which only the app has; other URLs load as they are.
      resolveFileUrl: (url: string) => {
        if (!url.startsWith('/')) return Promise.resolve(url)
        let file = files.get(url)
        if (!file) {
          file = actions.current.loadFile(url).then((data) => data ?? url)
          files.set(url, file)
        }
        return file
      },
    }),
  )

  useEffect(() => watchDeletedFiles(editor, (url) => actions.current.fileDeleted(url)), [editor])

  // The screen fetches what each database card shows; tell it which databases the document holds, on each change.
  useEffect(() => {
    let last = ''
    const report = () => {
      const blocks = databaseBlocks(editor.document)
      const json = JSON.stringify(blocks)
      if (json === last) return
      last = json
      void actions.current.onDatabases(blocks)
    }
    report()
    return editor.onChange(report)
  }, [editor, actions])

  // The view mounts once, with the first value; later changes switch the mounted editor (see `setEditableInPlace`).
  const [mountEditable] = useState(editable)
  useEffect(() => setEditableInPlace(editor, editable), [editor, editable])

  useEffect(() => {
    editorRef.current = editor
    let last = ''
    let focused = false
    let blurTimer: ReturnType<typeof setTimeout> | undefined
    const send = (state: FormatState) => {
      const json = JSON.stringify(state)
      if (json === last) return
      last = json
      focused = state.focused
      void actions.current.onFormat(state)
    }
    const report = () => {
      clearTimeout(blurTimer)
      const state = formatState(editor, rootRef.current)
      // A tap on the native toolbar can blur the editor for a moment (the web target); its command focuses it again.
      if (focused && !state.focused) blurTimer = setTimeout(() => send(formatState(editor, rootRef.current)), 250)
      else send(state)
    }
    const offSelection = editor.onSelectionChange(report)
    const offFocus = editor.onFocusChange(report)
    const offChange = editor.onChange(report)
    return () => {
      offSelection()
      offFocus()
      offChange()
      clearTimeout(blurTimer)
      if (editorRef.current === editor) editorRef.current = null
    }
  }, [editor, editorRef, actions])

  return (
    <div ref={rootRef} className="orbit-editor-root">
      <BlockNoteViewRaw
        editor={editor}
        editable={mountEditable}
        theme={theme}
        formattingToolbar={false}
        linkToolbar={false}
        slashMenu={false}
        sideMenu={false}
        filePanel={false}
        tableHandles={false}
        emojiPicker={false}
        comments={false}
      />
    </div>
  )
}

function formatState(editor: PageEditorInstance, root: HTMLElement | null): FormatState {
  const focused = editor.isFocused()
  const active = editor.getActiveStyles() as Record<string, unknown>
  const styles = Object.fromEntries(INLINE_STYLES.map((style) => [style, active[style] === true])) as FormatState['styles']
  let block: FormatState['block'] = null
  let caret: FormatState['caret'] = null
  try {
    block = blockKindOf(editor.getTextCursorPosition().block as { type: string; props?: Record<string, unknown> })
    const view = editor.prosemirrorView
    if (view && root) {
      const coords = view.coordsAtPos(view.state.selection.head)
      const top = root.getBoundingClientRect().top
      caret = { top: Math.round(coords.top - top), bottom: Math.round(coords.bottom - top) }
    }
  } catch {
    // no text selection (a whole block is selected)
  }
  return { focused, styles, block, canIndent: editor.canNestBlock(), canOutdent: editor.canUnnestBlock(), caret }
}

function runCommand(editor: PageEditorInstance, command: EditorCommand) {
  if (!editor.isEditable) return
  switch (command.type) {
    case 'style':
      editor.toggleStyles({ [command.style]: true } as never)
      break
    case 'block': {
      const spec = BLOCK_KINDS[command.kind]
      const blocks = editor.getSelection()?.blocks ?? [editor.getTextCursorPosition().block]
      const update = { type: spec.type, props: 'props' in spec ? spec.props : {} }
      for (const block of blocks) editor.updateBlock(block, update as never)
      break
    }
    case 'indent':
      if (editor.canNestBlock()) editor.nestBlock()
      break
    case 'outdent':
      if (editor.canUnnestBlock()) editor.unnestBlock()
      break
    case 'undo':
      editor.undo()
      break
    case 'redo':
      editor.redo()
      break
    case 'image':
      insertAfterCursor(editor, { type: 'image', props: { url: command.url, name: command.name } })
      break
    case 'divider':
      insertAfterCursor(editor, { type: 'divider' })
      break
    case 'blur':
      editor.blur()
      return
  }
  editor.focus()
}

/** Inserts `block` after the cursor's block and puts the cursor in a new paragraph after it. */
function insertAfterCursor(editor: PageEditorInstance, block: Record<string, unknown>) {
  const at = editor.getTextCursorPosition().block
  const [inserted] = editor.insertBlocks([block as never, { type: 'paragraph' }], at, 'after')
  const next = inserted ? editor.getNextBlock(inserted) : undefined
  if (next) editor.setTextCursorPosition(next, 'start')
}
