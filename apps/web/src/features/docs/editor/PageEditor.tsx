import '@blocknote/shadcn/style.css'
import './PageEditor.css'

import { use, useEffect, useImperativeHandle, useMemo, useRef, useState, useSyncExternalStore, type MouseEvent, type Ref } from 'react'
import { createPortal } from 'react-dom'
import { QueryClientContext, type QueryClient } from '@tanstack/react-query'
import { CommentsExtension } from '@blocknote/core/comments'
import { en } from '@blocknote/core/locales'
import { withCollaboration } from '@blocknote/core/yjs'
import {
  ComponentsContext,
  FloatingComposerController,
  FloatingThreadController,
  SuggestionMenuController,
  useCreateBlockNote,
} from '@blocknote/react'
import { BlockNoteView } from '@blocknote/shadcn'
import type { Awareness } from 'y-protocols/awareness'
import type * as Y from 'yjs'
import { Spinner } from '@/components/ui/spinner'
import { useTheme } from '@/lib/themeContext'
import { internalPageLinkId, isSafeLinkHref, toEditorContent, toStoredContent } from './content'
import { insertPageBlock } from './pageBlockCommands'
import { PageEditorContext, type PageEditorContextValue, type PageRef } from './pageEditorContext'
import { EDITOR_BLOCK_TYPES, pageEditorSchema, type PageEditorInstance } from './schema'
import { getPageSlashMenuItems } from './slashMenu'
import { PageMentionMenu } from './MentionMenu'
import { PageMentionNamesContext, type PageMentionOptions } from './pageMentions'
import { commentsApi, threadsQueryOptions } from '../comments/api'
import { commentComponents } from '../comments/components'
import { commentEditorSchema, MentionCandidatesContext, MentionNamesContext, type MentionCandidate } from '../comments/mentions'
import { CommentMembersContext } from '../comments/mentionContext'
import { COMMENT_PLACEHOLDERS } from '../comments/dictionary'
import { COMMENT_POPUP_ATTR, FloatingThreadCard, NewCommentCard } from '../comments/ThreadCard'
import { ThreadList } from '../comments/ThreadList'
import { NoCommentsThreadStore, OrbitThreadStore, type OrbitThreadStoreOptions } from '../comments/threadStore'

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

/** Comments on the page (collaborative editors only): threads anchored to text through BlockNote's comment marks. */
export interface PageEditorComments {
  workspaceId: string
  pageId: string
  /** The signed-in user (null while loading: nothing can be changed yet). */
  userId: string | null
  /** Workspace members, for comment authors and mention names. `null` while loading. */
  members: readonly MentionCandidate[] | null
  /** Who the "@" picker offers (members who can see the page). */
  mentionable: readonly MentionCandidate[]
  /** Where the Comments panel's thread list renders (null: panel closed). */
  panel: { container: HTMLElement; filter: 'open' | 'resolved' } | null
  /** Select (and scroll to) this thread once it is loaded; `onThreadShown` fires after. */
  showThreadId?: string | null
  onThreadShown?: () => void
  onError?: OrbitThreadStoreOptions['onError']
}


/** "Comment" in the formatting toolbar (BlockNote says "Add comment"); Orbit's comment field placeholders. */
const dictionary = {
  ...en,
  formatting_toolbar: { ...en.formatting_toolbar, comment: { tooltip: 'Comment' } },
  placeholders: { ...en.placeholders, ...COMMENT_PLACEHOLDERS },
}

/** The floating thread ignores presses in popups opened from it (their menus portal outside the card). */
const floatingThreadOptions = {
  useDismissProps: {
    outsidePress: (event: globalThis.MouseEvent) => !(event.target instanceof Element && event.target.closest(`[${COMMENT_POPUP_ATTR}]`)),
  },
}

const FORMER_MEMBER = 'Former member'

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
  /** Comments (collab only). Collaborative editors always load the comment mark: without it y-prosemirror would drop
   *  the text of anchored comments for everyone. Omitted, comments are shown as unavailable. */
  comments?: PageEditorComments
  /** @mentions in the body: who "@" offers and the member names chips show. Without it "@" offers nobody. */
  mentions?: PageMentionOptions
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
  comments,
  mentions,
  className,
  ref,
}: PageEditorProps) {
  const { theme } = useTheme()
  // Optional: read-only previews render without a query client (and without comments).
  const queryClient = use(QueryClientContext)
  const [latest] = useState(() => new Latest(comments))
  useEffect(() => {
    latest.setComments(comments)
  })
  // One store per editor instance (the editor is keyed by page and document).
  const [threadStore] = useState(() => createThreadStore(comments, queryClient, latest))
  // BlockNote caches resolved users, so wait for the member list instead of answering "unknown" early.
  const [membersReady] = useState(() => {
    let resolve: () => void = () => {}
    const promise = new Promise<void>((done) => {
      resolve = done
    })
    return { promise, resolve }
  })
  useEffect(() => {
    if (comments?.members) membersReady.resolve()
  }, [comments?.members, membersReady])
  // Collaborative editors mount once the page's threads are known (see `OrbitThreadStore.isSettled`).
  const threadsSettled = useSyncExternalStore(threadStore.subscribeSettled, threadStore.isSettled)
  const names = useMemo(() => new Map((comments?.members ?? []).map((member) => [member.id, member.name])), [comments?.members])
  const membersById = useMemo(() => new Map((comments?.members ?? []).map((member) => [member.id, member])), [comments?.members])
  const mounted = useRef(true)
  const uploadRef = useRef(uploadFile)
  const openPageRef = useRef(onOpenPage)
  const mentionsRef = useRef(mentions)
  useEffect(() => {
    mentionsRef.current = mentions
  }, [mentions])
  const mentionMembers = mentions?.members
  const mentionNames = useMemo(
    () => (mentionMembers ? new Map(mentionMembers.map((member) => [member.id, member.name])) : null),
    [mentionMembers],
  )
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
    dictionary,
  }
  const commentsExtension = CommentsExtension({
    threadStore,
    schema: commentEditorSchema,
    resolveUsers: userResolver(membersReady.promise, latest),
    // Comment fields are multi-line: Enter is a new line, Cmd/Ctrl+Enter sends.
    submitOnEnter: false,
  })
  const editor: PageEditorInstance = useCreateBlockNote(
    collab
      ? // The server initializes every document (never an empty fragment), so there is no initialContent here;
        // withCollaboration also swaps the undo history for Yjs' (only local edits are undone).
        withCollaboration({
          ...options,
          extensions: [commentsExtension],
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

  useEffect(() => {
    latest.setEditor(editor)
  }, [latest, editor])

  // Deep link to a thread (inbox): select it once the threads are loaded.
  const showThreadId = comments?.showThreadId ?? null
  useEffect(() => {
    if (!collab || !showThreadId) return
    const extension = editor.getExtension(CommentsExtension)
    if (!extension) return
    const show = () => {
      if (!threadStore.getThreads().has(showThreadId)) return false
      extension.selectThread(showThreadId)
      latest.current()?.onThreadShown?.()
      return true
    }
    if (show()) return
    return threadStore.subscribe(() => void show())
  }, [collab, editor, latest, showThreadId, threadStore])

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

  if (collab && !threadsSettled) {
    return (
      <div className="flex justify-center py-10" data-testid="editor-loading">
        <Spinner className="text-muted-foreground" />
      </div>
    )
  }

  return (
    <PageEditorContext value={context}>
      <PageMentionNamesContext value={mentionNames}>
      <div className="contents" onClickCapture={onClickCapture}>
        <BlockNoteView
          editor={editor}
          editable={editable}
          theme={theme}
          className={className ? `orbit-page-editor ${className}` : 'orbit-page-editor'}
          slashMenu={false}
          comments={false}
          onChange={onChange ? (changed) => onChange(toStoredContent(changed.document)) : undefined}
        >
          <SuggestionMenuController
            triggerCharacter="/"
            getItems={async (query) => getPageSlashMenuItems(editor, slashActions, query)}
          />
          <PageMentionMenu editor={editor} options={() => mentionsRef.current} />
          {collab ? (
            <MentionNamesContext value={names}>
              <MentionCandidatesContext value={comments?.mentionable ?? []}>
                <CommentMembersContext value={membersById}>
                <ComponentsContext.Provider value={commentComponents}>
                  <FloatingComposerController floatingComposer={NewCommentCard} />
                  {/* With the Comments panel open, threads show there only (a floating card for the same selected
                      thread would fight the panel for focus and dismiss itself on every click in the panel). */}
                  {comments?.panel ? null : <FloatingThreadController floatingThread={FloatingThreadCard} floatingUIOptions={floatingThreadOptions} />}
                  {comments?.panel
                    ? createPortal(
                        <div className="bn-shadcn orbit-comments-list" data-color-scheme={theme} data-filter={comments.panel.filter}>
                          <ThreadList filter={comments.panel.filter} />
                        </div>,
                        comments.panel.container,
                      )
                    : null}
                </ComponentsContext.Provider>
                </CommentMembersContext>
              </MentionCandidatesContext>
            </MentionNamesContext>
          ) : null}
        </BlockNoteView>
      </div>
      </PageMentionNamesContext>
    </PageEditorContext>
  )
}

/** The latest comments props and editor, for the thread store's callbacks (the store outlives renders). */
class Latest {
  private comments: PageEditorComments | undefined
  private editor: PageEditorInstance | null = null

  constructor(comments: PageEditorComments | undefined) {
    this.comments = comments
  }

  setComments(comments: PageEditorComments | undefined) {
    this.comments = comments
  }

  setEditor(editor: PageEditorInstance) {
    this.editor = editor
  }

  current() {
    return this.comments
  }

  currentEditor() {
    return this.editor
  }
}

function createThreadStore(
  comments: PageEditorComments | undefined,
  queryClient: QueryClient | undefined,
  latest: Latest,
) {
  if (!comments || !queryClient) return new NoCommentsThreadStore()
  return new OrbitThreadStore({
    api: commentsApi(comments.workspaceId, comments.pageId),
    queryClient,
    query: threadsQueryOptions(comments.workspaceId, comments.pageId),
    userId: () => latest.current()?.userId ?? null,
    quote: () => {
      const state = latest.currentEditor()?.prosemirrorState
      return state ? state.doc.textBetween(state.selection.from, state.selection.to, ' ') : ''
    },
    onError: (action, error) => latest.current()?.onError?.(action, error),
  })
}

/** Comment authors by id (BlockNote caches the answer, so it waits for the member list first). */
function userResolver(membersLoaded: Promise<void>, latest: Latest) {
  return async (userIds: string[]) => {
    await membersLoaded
    const members = new Map((latest.current()?.members ?? []).map((member) => [member.id, member]))
    return userIds.map((id) => ({ id, username: members.get(id)?.name ?? FORMER_MEMBER, avatarUrl: '' }))
  }
}

/** The BlockNote link anchor an event happened in, if any. */
function linkAnchor(target: EventTarget | null): HTMLAnchorElement | null {
  return target instanceof Element ? target.closest<HTMLAnchorElement>('a[data-inline-content-type="link"]') : null
}

function opensNewTab(event: { metaKey: boolean; ctrlKey: boolean; shiftKey: boolean }): boolean {
  return event.metaKey || event.ctrlKey || event.shiftKey
}
