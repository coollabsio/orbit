import { useCommand } from '@/shortcuts/useCommand'
import '@blocknote/shadcn/style.css'

import { use, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, useSyncExternalStore, type MouseEvent, type Ref } from 'react'
import { createPortal } from 'react-dom'
import { QueryClientContext, type QueryClient } from '@tanstack/react-query'
import { CommentsExtension } from '@blocknote/core/comments'
import { en } from '@blocknote/core/locales'
import { withCollaboration } from '@blocknote/core/yjs'
import {
  ComponentsContext,
  FloatingComposerController,
  FloatingThreadController,
  FormattingToolbar,
  FormattingToolbarController,
  getFormattingToolbarItems,
  SuggestionMenuController,
  useCreateBlockNote,
} from '@blocknote/react'
import type { Awareness } from 'y-protocols/awareness'
import type * as Y from 'yjs'
import { ImageViewer } from '@/components/common/ImageViewer'
import { Spinner } from '@/components/ui/spinner'
import type { Attachment } from '@/lib/attachmentLib'
import { useTheme } from '@/lib/themeContext'
import { cn } from 'cn'
import { ImageZoomButton } from './ImageZoomButton'
import { internalPageLinkId, isSafeLinkHref, toEditorContent, toStoredContent } from './content'
import { insertPageBlock } from './pageBlockCommands'
import { PageEditorContext, type PageEditorContextValue, type PageRef } from './pageEditorContext'
import { EDITOR_BLOCK_TYPES, pageEditorSchema, type PageEditorInstance } from './schema'
import { getPageSlashMenuItems } from './slashMenu'
import { PageMentionMenu } from './MentionMenu'
import { PageMentionNamesContext, type PageMentionOptions } from './pageMentions'
import { ThemedBlockNoteView } from './ThemedBlockNoteView'
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
  // Lives here, not in the toolbar: the toolbar unmounts when the viewer takes focus from the editor.
  const [zoomImage, setZoomImage] = useState<Attachment | null>(null)
  const formattingToolbar = useCallback(() => {
    const items = getFormattingToolbarItems()
    const at = items.findIndex((item) => item.key === 'fileDownloadButton') + 1
    items.splice(at, 0, <ImageZoomButton key="imageZoomButton" onZoom={setZoomImage} />)
    return <FormattingToolbar>{items}</FormattingToolbar>
  }, [])
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
    // blocks. The same rules ship statically at the end of src/index.css instead.
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

  // a comment needs selected text to attach to; the key works from inside the editor
  useCommand('docs.comment', collab && comments ? () => editor.getExtension(CommentsExtension)?.startPendingComment() : null, { available: () => editor.getSelectedText().trim() !== '' })

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
        <ThemedBlockNoteView
          editor={editor}
          editable={editable}
          theme={theme}
          data-slot="page-editor"
          className={cn(
            // Static placeholders: BlockNote injects its own through a runtime <style>, which the production CSP
            // (`style-src 'self'`) blocks. The generic hint sits in :where() so the block-type ones win over it.
            `[&>.bn-editor[contenteditable=true]_.bn-block-content:where([data-is-empty-and-focused]):has(.ProseMirror-trailingBreak:only-child)]:after:content-["Enter_text_or_type_'/'_for_commands"]!`,
            "[&>.bn-editor[contenteditable=true]_.bn-block-content[data-content-type=heading]:has(.ProseMirror-trailingBreak:only-child)]:after:content-['Heading']!",
            "[&>.bn-editor[contenteditable=true]_.bn-block-content[data-content-type=toggleListItem]:has(.ProseMirror-trailingBreak:only-child)]:after:content-['Toggle']!",
            "[&>.bn-editor[contenteditable=true]_.bn-block-content:is([data-content-type=bulletListItem],[data-content-type=numberedListItem],[data-content-type=checkListItem]):has(.ProseMirror-trailingBreak:only-child)]:after:content-['List']!",
            // The callout's placeholder sits next to the emoji (BlockNote's would follow the whole row).
            "[&>.bn-editor[contenteditable=true]_.bn-block-content[data-content-type=callout]:has(.ProseMirror-trailingBreak:only-child)]:after:content-['Callout']!",
            '[&>.bn-editor[contenteditable=true]_.bn-block-content[data-content-type=callout]:has(.ProseMirror-trailingBreak:only-child)]:after:absolute [&>.bn-editor[contenteditable=true]_.bn-block-content[data-content-type=callout]:has(.ProseMirror-trailingBreak:only-child)]:after:top-px [&>.bn-editor[contenteditable=true]_.bn-block-content[data-content-type=callout]:has(.ProseMirror-trailingBreak:only-child)]:after:left-8',
            // Inline links use the app's link style instead of the browser's blue.
            '[&_.bn-editor_a[data-inline-content-type=link]]:text-primary! [&_.bn-editor_a[data-inline-content-type=link]]:underline! [&_.bn-editor_a[data-inline-content-type=link]]:underline-offset-2!',
            // Callout box (CalloutBlock.tsx): the whole `.bn-block`, so nested blocks sit inside it like in Notion. React
            // node views put a `.react-renderer` between `.bn-block` and the content, hence both forms in `:has()`.
            // The tint is mixed here instead of BlockNote's color rules (saturated in dark mode): full on light, a faint
            // wash on dark. No `data-background-color` = gray (the default); `default` = no fill, just a border.
            '[&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:[--callout-tint:var(--bn-colors-highlights-gray-background)] [&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:[--callout-strength:100%]',
            'data-[color-scheme=dark]:[&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:[--callout-strength:24%]',
            '[&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:my-1 [&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:rounded-[6px] [&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:border [&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:border-transparent [&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:py-3 [&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:pr-3.5 [&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:pl-3',
            '[&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])]:bg-[color-mix(in_oklab,var(--callout-tint)_var(--callout-strength),transparent)]!',
            '[&_.bn-block:has(>[data-content-type=callout][data-background-color=brown],>*>[data-content-type=callout][data-background-color=brown])]:[--callout-tint:var(--bn-colors-highlights-brown-background)]',
            '[&_.bn-block:has(>[data-content-type=callout][data-background-color=red],>*>[data-content-type=callout][data-background-color=red])]:[--callout-tint:var(--bn-colors-highlights-red-background)]',
            '[&_.bn-block:has(>[data-content-type=callout][data-background-color=orange],>*>[data-content-type=callout][data-background-color=orange])]:[--callout-tint:var(--bn-colors-highlights-orange-background)]',
            '[&_.bn-block:has(>[data-content-type=callout][data-background-color=yellow],>*>[data-content-type=callout][data-background-color=yellow])]:[--callout-tint:var(--bn-colors-highlights-yellow-background)]',
            '[&_.bn-block:has(>[data-content-type=callout][data-background-color=green],>*>[data-content-type=callout][data-background-color=green])]:[--callout-tint:var(--bn-colors-highlights-green-background)]',
            '[&_.bn-block:has(>[data-content-type=callout][data-background-color=blue],>*>[data-content-type=callout][data-background-color=blue])]:[--callout-tint:var(--bn-colors-highlights-blue-background)]',
            '[&_.bn-block:has(>[data-content-type=callout][data-background-color=purple],>*>[data-content-type=callout][data-background-color=purple])]:[--callout-tint:var(--bn-colors-highlights-purple-background)]',
            '[&_.bn-block:has(>[data-content-type=callout][data-background-color=pink],>*>[data-content-type=callout][data-background-color=pink])]:[--callout-tint:var(--bn-colors-highlights-pink-background)]',
            '[&_.bn-block:has(>[data-content-type=callout][data-background-color=default],>*>[data-content-type=callout][data-background-color=default])]:border-border [&_.bn-block:has(>[data-content-type=callout][data-background-color=default],>*>[data-content-type=callout][data-background-color=default])]:bg-transparent!',
            '[&_.bn-block-content[data-content-type=callout]]:relative [&_.bn-block-content[data-content-type=callout]]:p-0! [&_.bn-block-content[data-content-type=callout]]:bg-transparent!',
            // Nested blocks line up with the callout text (emoji column + gap), not with the emoji.
            '[&_.bn-block:has(>[data-content-type=callout],>*>[data-content-type=callout])>.bn-block-group]:ml-[32px]!',
            // Comment anchors: a soft amber wash with an underline, stronger for the selected thread.
            '[&_.bn-thread-mark:not([data-orphan=true])]:cursor-pointer [&_.bn-thread-mark:not([data-orphan=true])]:border-b-2 [&_.bn-thread-mark:not([data-orphan=true])]:border-b-[rgb(245_158_11/0.55)] [&_.bn-thread-mark:not([data-orphan=true])]:bg-[rgb(251_191_36/0.2)]!',
            '[&_.bn-thread-mark-selected]:bg-[rgb(251_191_36/0.45)]!',
            'data-[color-scheme=dark]:[&_.bn-thread-mark:not([data-orphan=true])]:border-b-[rgb(251_191_36/0.5)] data-[color-scheme=dark]:[&_.bn-thread-mark:not([data-orphan=true])]:bg-[rgb(251_191_36/0.13)]!',
            'data-[color-scheme=dark]:[&_.bn-thread-mark-selected]:bg-[rgb(251_191_36/0.32)]!',
            // Phones have no room for BlockNote's 54px side-menu gutter; blocks use the full width.
            'max-[900px]:[&>.bn-editor]:px-0!',
            // Formatting toolbar on phones: BlockNote caps it at 100vw, which runs off the right edge. Keep a margin,
            // scroll the rest sideways, and use slightly smaller buttons so more fit.
            'max-[900px]:[&_.bn-formatting-toolbar]:max-w-[calc(100vw-16px)]! max-[900px]:[&_.bn-formatting-toolbar]:gap-0! max-[900px]:[&_.bn-formatting-toolbar]:overscroll-x-contain max-[900px]:[&_.bn-formatting-toolbar]:[scrollbar-width:none]',
            'max-[900px]:[&_.bn-formatting-toolbar>button]:h-8! max-[900px]:[&_.bn-formatting-toolbar>button]:min-w-8! max-[900px]:[&_.bn-formatting-toolbar>button]:shrink-0! max-[900px]:[&_.bn-formatting-toolbar>button]:px-0!',
            className,
          )}
          slashMenu={false}
          formattingToolbar={false}
          comments={false}
          onChange={onChange ? (changed) => onChange(toStoredContent(changed.document)) : undefined}
        >
          <SuggestionMenuController
            triggerCharacter="/"
            getItems={async (query) => getPageSlashMenuItems(editor, slashActions, query)}
          />
          <FormattingToolbarController formattingToolbar={formattingToolbar} />
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
                        <div className="bn-shadcn" data-slot="comment-thread-list" data-color-scheme={theme} data-filter={comments.panel.filter}>
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
        </ThemedBlockNoteView>
        {zoomImage ? <ImageViewer attachment={zoomImage} onClose={() => setZoomImage(null)} /> : null}
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
