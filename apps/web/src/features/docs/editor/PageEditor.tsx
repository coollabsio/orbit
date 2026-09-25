import '@blocknote/shadcn/style.css'
import './PageEditor.css'

import { useEffect, useImperativeHandle, useMemo, useRef, type MouseEvent, type Ref } from 'react'
import { withCollaboration } from '@blocknote/core/yjs'
import { SuggestionMenuController, useCreateBlockNote } from '@blocknote/react'
import { BlockNoteView } from '@blocknote/shadcn'
import type { Awareness } from 'y-protocols/awareness'
import type * as Y from 'yjs'
import { useTheme } from '@/lib/themeContext'
import { internalPageLinkId, isSafeLinkHref, toEditorContent, toStoredContent } from './content'
import { insertPageBlock } from './pageBlockCommands'
import { PageEditorContext, type PageEditorContextValue, type PageRef } from './pageEditorContext'
import { EDITOR_BLOCK_TYPES, pageEditorSchema, type PageEditorInstance } from './schema'
import { getPageSlashMenuItems } from './slashMenu'

export type { PageRef } from './pageEditorContext'

export interface PageEditorHandle {
  /** Current document as sanitized JSON (same shape `onChange` receives). */
  getContent: () => unknown[]
  focus: () => void
}

/** Real-time co-editing: the document lives in `fragment` (already synced with the server) instead of `initialContent`. */
export interface PageEditorCollab {
  provider: { awareness?: Awareness }
  fragment: Y.XmlFragment
  /** Local caret owner (the server stamps the session's user for everyone else). */
  user: { name: string; color: string }
}

export interface PageEditorProps {
  /** The editor is recreated (fresh document + undo history) whenever this changes. */
  pageId: string
  /** `Page.content` from the API, read once per `pageId`. Ignored with `collab` (the Y document is the content). */
  initialContent?: unknown[]
  /** Bind to a collaborative document. Fixed per editor instance: key the editor to switch documents. */
  collab?: PageEditorCollab
  editable: boolean
  /** Called on every edit with the full sanitized document (read-only previews and tests; collab needs none). */
  onChange?: (content: unknown[]) => void
  resolvePage: (pageId: string) => PageRef | null
  onOpenPage: (pageId: string) => void
  /** Creates a child page of `pageId`; resolves to the new page id (reject/throw to abort). */
  onCreateSubpage: () => Promise<string>
  /** Lets the user pick an existing page; resolves to its id, or null when cancelled. */
  onPickPage: () => Promise<string | null>
  /**
   * Uploads a file for an image/file block (slash menu, file panel, paste, drop) and resolves to its URL.
   * Reject to abort; the caller reports the error. Without it the editor offers no uploads.
   */
  uploadFile?: (file: File) => Promise<string>
  className?: string
  ref?: Ref<PageEditorHandle>
}

export function PageEditor(props: PageEditorProps) {
  // A new key per page gives a fresh BlockNote instance, so content and undo history never leak across pages.
  return <PageEditorInner key={props.pageId} {...props} />
}

function PageEditorInner({
  initialContent,
  collab,
  editable,
  onChange,
  resolvePage,
  onOpenPage,
  onCreateSubpage,
  onPickPage,
  uploadFile,
  className,
  ref,
}: PageEditorProps) {
  const { theme } = useTheme()
  const mounted = useRef(true)
  const uploadRef = useRef(uploadFile)
  const openPageRef = useRef(onOpenPage)
  useEffect(() => {
    uploadRef.current = uploadFile
    openPageRef.current = onOpenPage
  }, [uploadFile, onOpenPage])

  const options = {
    schema: pageEditorSchema,
    links: {
      isValidLink: isSafeLinkHref,
      // Edit mode: BlockNote calls this instead of `window.open`. In-app page links are handled by the click
      // listener below (it also covers read-only mode); everything else opens in a new tab as before.
      onClick: (event: globalThis.MouseEvent) => {
        const anchor = linkAnchor(event.target)
        const href = anchor?.getAttribute('href')
        if (!href || !isSafeLinkHref(href)) return true
        if (internalPageLinkId(href) && !opensNewTab(event)) return true
        window.open(href, '_blank', 'noopener,noreferrer')
        return true
      },
    },
    // Whether uploads exist is fixed per editor; the latest callback is read through a ref.
    uploadFile: uploadFile ? (file: File) => uploadRef.current!(file) : undefined,
    // Tiptap would inject its base stylesheet as a <style> tag, which the production CSP (`style-src 'self'`)
    // blocks. The same rules ship statically in PageEditor.css instead.
    _tiptapOptions: { injectCSS: false },
  }
  const editor: PageEditorInstance = useCreateBlockNote(
    collab
      ? // The server initializes every document (never an empty fragment), so there is no initialContent here;
        // withCollaboration also swaps the undo history for Yjs' (only local edits are undone).
        withCollaboration({
          ...options,
          collaboration: { provider: collab.provider, fragment: collab.fragment, user: collab.user, showCursorLabels: 'activity' },
        })
      : {
          ...options,
          // `[]` (new page) becomes undefined → BlockNote starts with one empty paragraph.
          initialContent: toEditorContent(initialContent ?? [], EDITOR_BLOCK_TYPES) as never,
        },
  )

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])

  useImperativeHandle(
    ref,
    () => ({
      getContent: () => toStoredContent(editor.document),
      focus: () => editor.focus(),
    }),
    [editor],
  )

  const context = useMemo<PageEditorContextValue>(() => ({ resolvePage, onOpenPage }), [resolvePage, onOpenPage])

  const slashActions = {
    onSubpage: () => {
      const anchorId = editor.getTextCursorPosition().block.id
      void onCreateSubpage().then(
        (pageId) => {
          if (!mounted.current) return
          insertPageBlock(editor, anchorId, pageId)
          onOpenPage(pageId)
        },
        () => {
          // Creation failed; the caller reports the error. Nothing to insert.
        },
      )
    },
    onLinkPage: () => {
      const anchorId = editor.getTextCursorPosition().block.id
      void onPickPage().then(
        (pageId) => {
          if (pageId && mounted.current) insertPageBlock(editor, anchorId, pageId)
        },
        () => {},
      )
    },
  }

  /** `/docs/<uuid>` links navigate in the app (no reload, no new tab) unless a modifier asks for a new tab. */
  const onClickCapture = (event: MouseEvent<HTMLDivElement>) => {
    const anchor = linkAnchor(event.target)
    const pageId = internalPageLinkId(anchor?.getAttribute('href'))
    if (!pageId || event.button !== 0 || opensNewTab(event.nativeEvent)) return
    event.preventDefault()
    openPageRef.current(pageId)
  }

  return (
    <PageEditorContext value={context}>
      <div className="contents" onClickCapture={onClickCapture}>
        <BlockNoteView
          editor={editor}
          editable={editable}
          theme={theme}
          className={className ? `orbit-page-editor ${className}` : 'orbit-page-editor'}
          slashMenu={false}
          onChange={onChange ? (changed) => onChange(toStoredContent(changed.document)) : undefined}
        >
          <SuggestionMenuController
            triggerCharacter="/"
            getItems={async (query) => getPageSlashMenuItems(editor, slashActions, query)}
          />
        </BlockNoteView>
      </div>
    </PageEditorContext>
  )
}

/** The BlockNote link anchor an event happened in, if any. */
function linkAnchor(target: EventTarget | null): HTMLAnchorElement | null {
  return target instanceof Element ? target.closest<HTMLAnchorElement>('a[data-inline-content-type="link"]') : null
}

function opensNewTab(event: { metaKey: boolean; ctrlKey: boolean; shiftKey: boolean }): boolean {
  return event.metaKey || event.ctrlKey || event.shiftKey
}
