import { lazy, Suspense, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type Ref } from 'react'
import { Link, useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { ArrowLeft, Check, Copy, MoreH as Ellipsis, History, Lock, Star, Trash as Trash2, People as Users } from 'reicon-react'
import { cn } from 'cn'
import { apiClient, UNAUTHORIZED_EVENT } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import type { Page, PageSummary, PageVersionSummary, Teamspace } from '@/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Spinner } from '@/components/ui/spinner'
import { Emoji } from '@/components/common/Emoji'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import {
  conflictCurrentPage,
  conflictCurrentVersion,
  fetchPage,
  isPageNotFound,
  isPageVersionConflict,
  pageUploadErrorMessage,
  patchTreePage,
  reconcilePage,
  savePage,
  uploadPageFileRequest,
  useCreatePage,
} from '@/features/docs/api/pages'
import { usePageFavorites, useToggleFavorite } from '@/features/docs/api/favorites'
import { useRestorePageVersion } from '@/features/docs/api/pageVersions'
import { useCurrentUser } from '@/features/auth/api'
import { PageAutosaver, type AutosaveState, type PagePatch } from '@/features/docs/autosave'
import { collabColor } from '@/features/docs/collab/palette'
import { useCollaborators } from '@/features/docs/collab/presence'
import { PresenceAvatars } from '@/features/docs/collab/PresenceAvatars'
import { collabStatusLabel, type CollabProblem, type CollabSession, type CollabState } from '@/features/docs/collab/session'
import { useCollabSession } from '@/features/docs/collab/useCollabSession'
import { useDocPageActions } from '@/features/docs/pageActions'
import { PRIVATE_SPACE, ancestorsOf, pageTitle, removeSubtree, spaceKey, spaceLabel, teamspaceSpace, type SpaceKey } from '@/features/docs/pageTree'
import { dayLabel, timeOfDay } from '@/lib/format'
// Type-only imports: erased at build time, so the BlockNote chunk stays lazy.
import type { PageEditorHandle } from '@/features/docs/editor/PageEditor'
import type { PageRef } from '@/features/docs/editor/pageEditorContext'
import { CoverBanner } from './CoverBanner'
import { CoverSourcePanel } from './CoverSourcePanel'
import { menuItemClass } from './DocTreeItem'
import { PageHistoryPane } from './PageHistoryPane'
import { PageLinkDialog } from './PageLinkDialog'

// BlockNote + ProseMirror are ~1 MB: load them only when a page is opened.
const PageEditor = lazy(() => import('@/features/docs/editor/PageEditor').then((module) => ({ default: module.PageEditor })))

const editorSpinner = (
  <div className="flex justify-center py-10" data-testid="editor-loading">
    <Spinner className="text-muted-foreground" />
  </div>
)

/** The emoji picker panel brings its own chrome, so the popover is just an anchored frame. */
const emojiPanelClass = 'w-auto gap-0 rounded-xl border border-border bg-popover p-0 text-foreground shadow-xl ring-0'

/** What the docs page needs from the open editor to trash it safely. */
export interface DocEditorControl {
  /** Saves queued metadata edits and resolves with the version to send as `expected_version`. */
  flush: () => Promise<number>
  /** Drops queued edits and stops autosaving (the page is gone). */
  dispose: () => void
  /**
   * The user is trashing this page (or an ancestor) here: the socket's "page gone" close that follows is expected and
   * must not show the "moved to the trash" notice. Returns an undo for when the request fails.
   */
  expectGone: () => () => void
}

interface DocEditorProps {
  workspaceId: string
  /**
   * Live page query data: newer title/icon/cover from realtime refreshes are applied when there are no local edits.
   * Content is not taken from here after mount: it syncs through the collaborative document.
   */
  page: Page
  pages: PageSummary[]
  /** For the space breadcrumb and the "Move to" menu (undefined while loading). */
  teamspaces?: Teamspace[]
  onRequestTrash: (pageId: string) => void
  /** Moves the page (with its sub-pages) to the end of another space's root. */
  onRequestMove?: (pageId: string, space: SpaceKey) => void
  /** The page went away while open (trashed or out of reach elsewhere); the editor is leaving for /docs. */
  onGone?: (pageId: string) => void
  controlRef?: Ref<DocEditorControl>
}

type Cover = { url: string | null; position: string | null }

/** The REST-saved part of a page (content syncs live and is never compared here). */
function sameEditable(a: Page, b: Page): boolean {
  return a.title === b.title && a.icon === b.icon && a.cover_url === b.cover_url && a.cover_position === b.cover_position
}

const STATUS_DOT: Record<CollabState['status'], string> = {
  live: 'bg-emerald-500',
  connecting: 'bg-amber-400 animate-pulse',
  reset: 'bg-amber-400 animate-pulse',
  offline: 'bg-amber-500',
  lost: 'bg-destructive',
  error: 'bg-destructive',
}

/** Banners for sync problems the user has to act on. */
type SyncNotice = 'too-large' | 'protocol' | 'reconnect-failed' | null

export function DocEditor({ workspaceId, page, pages, teamspaces, onRequestTrash, onRequestMove, onGone, controlRef }: DocEditorProps) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const createPage = useCreatePage(workspaceId)
  const favoritesQuery = usePageFavorites(workspaceId)
  const toggleFavorite = useToggleFavorite(workspaceId)
  const editorRef = useRef<PageEditorHandle>(null)
  const navigateRef = useRef(navigate)
  useEffect(() => {
    navigateRef.current = navigate
  })
  const pageId = page.id
  const pageActions = useDocPageActions()
  const hasChildren = pages.some((item) => item.parent_id === pageId)

  const [title, setTitle] = useState(page.title)
  const [icon, setIcon] = useState(page.icon)
  const [cover, setCover] = useState<Cover>({ url: page.cover_url, position: page.cover_position })
  const [saveState, setSaveState] = useState<AutosaveState>({ status: 'idle', conflict: null, error: null })
  const [coverPanelOpen, setCoverPanelOpen] = useState(false)
  const [iconPickerOpen, setIconPickerOpen] = useState(false)
  const [addIconOpen, setAddIconOpen] = useState(false)
  const [linkOpen, setLinkOpen] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const restoreVersion = useRestorePageVersion(workspaceId, page.id)
  const pickResolver = useRef<((pageId: string | null) => void) | null>(null)
  /** Last server state the local copy is known to be based on. */
  const baseline = useRef(page)
  const titleRef = useRef(title)
  const me = useCurrentUser()
  const selfId = me.data?.id ?? null
  // Only the local awareness entry: the server stamps every peer's name and color from their session.
  const collabUser = useMemo(
    () => ({ name: me.data?.display_name ?? 'Me', color: collabColor(me.data?.id ?? '') }),
    [me.data?.display_name, me.data?.id],
  )
  /** Bumped after a 4409 reset (or "Reload page" on a sync error): drops the local document and reconnects. */
  const [generation, setGeneration] = useState(0)
  const [notice, setNotice] = useState<SyncNotice>(null)
  /** The page went away while open (trashed, access removed): the editor turns read-only until we leave. */
  const [lost, setLost] = useState(false)
  const expectGone = useRef(0)
  const sessionRef = useRef<CollabSession | null>(null)

  const [saver] = useState(
    () =>
      new PageAutosaver({
        version: page.version,
        save: (patch, expectedVersion) => savePage(apiClient, workspaceId, pageId, { ...patch, expected_version: expectedVersion }),
        onSaved: (saved) => {
          baseline.current = saved
          reconcilePage(queryClient, workspaceId, saved)
          // The tree follows the title being typed, not the one that was just saved.
          patchTreePage(queryClient, workspaceId, pageId, { title: titleRef.current })
        },
        onStateChange: setSaveState,
        conflictOf: (error) =>
          isPageVersionConflict(error) ? { currentVersion: conflictCurrentVersion(error), current: conflictCurrentPage(error) } : null,
        // Our edits sit on `baseline`; a server change that left those fields alone (e.g. a move) is safe to save over.
        rebaseOnto: (current) => {
          if (!sameEditable(current, baseline.current)) return false
          baseline.current = current
          return true
        },
      }),
  )

  /** Takes the server's title/icon/cover (content arrives through the collaborative document on its own). */
  const applyServerPage = useCallback(
    (server: Page) => {
      setTitle(server.title)
      titleRef.current = server.title
      setIcon(server.icon)
      setCover({ url: server.cover_url, position: server.cover_position })
      saver.adoptVersion(server.version)
      baseline.current = server
    },
    [saver],
  )

  // Remote metadata updates (realtime refetch, move, another tab): apply them unless there are local edits to protect.
  useEffect(() => {
    if (page.version <= saver.version) return
    if (sameEditable(page, baseline.current)) {
      // Only metadata changed (e.g. the page was moved): keep editing on top of the new version.
      saver.adoptVersion(page.version)
      baseline.current = page
      return
    }
    // With local edits the next save surfaces the conflict instead.
    if (saver.hasUnsavedChanges() || saver.state.status === 'conflict') return
    applyServerPage(page)
  }, [page, saver, applyServerPage])

  /** The page is gone for this user (trashed, moved out of reach): say so and leave for /docs. */
  const leavePage = (problem: 'gone' | 'forbidden') => {
    setLost(true)
    saver.dispose()
    // Our own "Move to trash" already navigates and toasts.
    if (expectGone.current > 0) return
    const name = titleRef.current.trim() || 'Untitled'
    toast.info(problem === 'gone' ? `“${name}” was moved to the trash.` : `You no longer have access to “${name}”.`)
    // Drop it from the tree now so /docs does not land on it again; the refetch brings the real state.
    queryClient.setQueryData(queryKeys.pages.tree(workspaceId), (tree: PageSummary[] | undefined) => tree && removeSubtree(tree, pageId))
    void queryClient.invalidateQueries({ queryKey: queryKeys.pages.tree(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: queryKeys.pages.favorites(workspaceId) })
    onGone?.(pageId)
    navigateRef.current('/docs', { replace: true })
  }

  /** 4409: the server's document was reset. Load the page again (new epoch) and open a fresh session. */
  const reopen = async () => {
    setNotice(null)
    try {
      const fresh = await fetchPage(apiClient, workspaceId, pageId)
      reconcilePage(queryClient, workspaceId, fresh)
      setGeneration((current) => current + 1)
    } catch (error) {
      if (isPageNotFound(error)) leavePage('gone')
      else setNotice('reconnect-failed')
    }
  }

  const onCollabProblem = (problem: CollabProblem) => {
    switch (problem) {
      case 'gone':
      case 'forbidden':
        leavePage(problem)
        break
      case 'session':
        // The same path as any 401 from the API: clear the session and go to the login page.
        window.dispatchEvent(new Event(UNAUTHORIZED_EVENT))
        break
      case 'reset':
        void reopen()
        break
      case 'rate-limited':
        toast.warning('Too many changes at once. Syncing again in a few seconds.', { id: `collab-rate-${pageId}` })
        break
      case 'too-large':
        setNotice('too-large')
        break
      case 'protocol':
        setNotice('protocol')
        break
    }
  }

  /** Handshakes keep failing before the upgrade: a REST read tells a trashed page (404) or ended session (401). */
  const probePage = async () => {
    try {
      await fetchPage(apiClient, workspaceId, pageId)
    } catch (error) {
      // A 401 is handled by the API client (login redirect); a 404 means the page is gone for us.
      if (isPageNotFound(error)) sessionRef.current?.fail('gone')
    }
  }

  const collab = useCollabSession({
    workspaceId,
    pageId,
    epoch: page.collab_epoch,
    generation,
    user: collabUser,
    onProblem: onCollabProblem,
    onProbe: () => void probePage(),
  })
  useEffect(() => {
    sessionRef.current = collab.session
  }, [collab.session])
  const people = useCollaborators(collab.connection?.provider.awareness ?? null, selfId)

  // Flush on unmount (route change, page switch) and when the tab is hidden or closed.
  useEffect(() => {
    const onBeforeUnload = (event: BeforeUnloadEvent) => {
      // Content edits made while offline live only in this tab (no offline storage yet).
      const unsyncedContent = sessionRef.current?.hasUnsyncedChanges() ?? false
      if (!saver.hasUnsavedChanges() && !unsyncedContent) return
      void saver.flush()
      event.preventDefault()
    }
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') void saver.flush()
    }
    window.addEventListener('beforeunload', onBeforeUnload)
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      window.removeEventListener('beforeunload', onBeforeUnload)
      document.removeEventListener('visibilitychange', onVisibility)
      // Nothing shows this editor's status after unmount, so report a failed final save here.
      void saver.flush().then(() => {
        if (!saver.hasUnsavedChanges()) return
        const name = titleRef.current.trim() || 'Untitled'
        toast.error(`Your last edits to “${name}” were not saved.`, {
          action: { label: 'Open', onClick: () => navigateRef.current(`/docs/${pageId}`) },
        })
      })
    }
    // `pageId` is fixed for this editor (it is keyed by page), and `navigate` is read through a ref: under
    // BrowserRouter it changes on every location change, which must not run this cleanup early.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [saver])

  useImperativeHandle(
    controlRef,
    () => ({
      flush: async () => {
        await saver.flush()
        return saver.version
      },
      dispose: () => saver.dispose(),
      expectGone: () => {
        expectGone.current += 1
        let undone = false
        return () => {
          if (undone) return
          undone = true
          expectGone.current -= 1
        }
      },
    }),
    [saver],
  )

  const changeTitle = (next: string) => {
    setTitle(next)
    titleRef.current = next
    saver.update({ title: next })
    patchTreePage(queryClient, workspaceId, pageId, { title: next })
  }

  const changeIcon = (next: string | null) => {
    setIcon(next)
    saver.update({ icon: next })
    patchTreePage(queryClient, workspaceId, pageId, { icon: next })
    void saver.flush()
  }

  const changeCover = (patch: { cover_url?: string | null; cover_position?: string | null }) => {
    setCover((current) => ({
      url: patch.cover_url !== undefined ? patch.cover_url : current.url,
      position: patch.cover_position !== undefined ? patch.cover_position : current.position,
    }))
    saver.update(patch)
    void saver.flush()
  }

  const reloadFromServer = async () => {
    try {
      const fresh = await fetchPage(apiClient, workspaceId, pageId)
      saver.discard(fresh.version)
      reconcilePage(queryClient, workspaceId, fresh)
      applyServerPage(fresh)
    } catch {
      toast.error('Could not load the latest version of this page.')
    }
  }

  const overwrite = () => {
    const local: PagePatch = { title, icon, cover_url: cover.url, cover_position: cover.position }
    void saver.overwrite(local)
  }

  /**
   * Restores a history version: saves pending title/icon/cover edits first (the restore must not race them), then
   * takes the restored metadata. The content needs nothing here: the server replaces the collaborative document and
   * every open editor (this one included) receives it live. Errors are toasted; resolves to whether it worked.
   */
  const restoreFromHistory = async (version: PageVersionSummary): Promise<boolean> => {
    await saver.flush()
    if (saver.hasUnsavedChanges() || saver.state.status === 'conflict' || saver.state.status === 'error') {
      toast.error('Your latest edits are not saved yet. Resolve that before restoring a version.')
      return false
    }
    try {
      const restored = await restoreVersion.mutateAsync({ versionId: version.id, expectedVersion: saver.version })
      // Title typing during the request is dropped: the restored page is the new base.
      saver.discard(restored.version)
      applyServerPage(restored)
      setHistoryOpen(false)
      toast.success(`Restored the version from ${dayLabel(version.created_at).toLowerCase()}, ${timeOfDay(version.created_at)}.`)
      return true
    } catch (error) {
      if (isPageVersionConflict(error)) {
        toast.error('This page changed in the meantime. Try again.')
        void queryClient.invalidateQueries({ queryKey: queryKeys.pages.detail(workspaceId, pageId) })
      } else {
        toast.error('Could not restore this version.')
      }
      return false
    }
  }

  const readCurrent = () => ({
    title: titleRef.current,
    icon,
    content: editorRef.current?.getContent() ?? page.content,
  })

  const byId = useMemo(() => new Map(pages.map((item) => [item.id, item])), [pages])
  // Trashed pages leave the tree, so anything not in it renders as "Missing page".
  const resolvePage = useCallback((id: string): PageRef | null => {
    const found = byId.get(id)
    return found ? { title: found.title, icon: found.icon, trashed: false } : null
  }, [byId])

  const onOpenPage = useCallback((id: string) => navigate(`/docs/${id}`), [navigate])

  const createSubpage = createPage.mutateAsync
  const onCreateSubpage = useCallback(async () => {
    try {
      const created = await createSubpage({ parent_id: pageId })
      return created.id
    } catch (error) {
      toast.error('Could not create the sub-page. Try again.')
      throw error
    }
  }, [createSubpage, pageId])

  /** Editor image/file uploads: errors are toasted, then rethrown so BlockNote drops the empty block. */
  const uploadFile = useCallback(
    async (file: File) => {
      try {
        return (await uploadPageFileRequest(apiClient, workspaceId, pageId, file)).url
      } catch (error) {
        toast.error(pageUploadErrorMessage(error))
        throw error
      }
    },
    [workspaceId, pageId],
  )

  /** Cover uploads show their error inside the cover panel. */
  const uploadCover = useCallback(
    async (file: File) => {
      try {
        return (await uploadPageFileRequest(apiClient, workspaceId, pageId, file)).url
      } catch (error) {
        throw new Error(pageUploadErrorMessage(error), { cause: error })
      }
    },
    [workspaceId, pageId],
  )

  const onPickPage = useCallback(
    () =>
      new Promise<string | null>((resolve) => {
        pickResolver.current?.(null)
        pickResolver.current = resolve
        setLinkOpen(true)
      }),
    [],
  )

  const closeLinkDialog = (picked: string | null) => {
    pickResolver.current?.(picked)
    pickResolver.current = null
    setLinkOpen(false)
  }

  const ancestors = ancestorsOf(pages, pageId)
  // The tree summary follows optimistic moves (and sub-pages moved with their root); the detail may lag.
  const space = spaceKey(byId.get(pageId) ?? page)
  const spaceTargets: SpaceKey[] = [...(teamspaces ?? []).map((teamspace) => teamspaceSpace(teamspace.id)), PRIVATE_SPACE]
  const status = saveState.status
  const syncStatus = lost ? 'lost' : collab.state.status
  // Content sync is the headline; failed or conflicting title/icon/cover saves take over the label.
  const statusLabel =
    status === 'error'
      ? 'Save failed'
      : status === 'conflict'
        ? 'Conflict'
        : syncStatus === 'live' && status === 'saving'
          ? 'Saving…'
          : collabStatusLabel({ ...collab.state, status: syncStatus })
  const statusTitle =
    syncStatus === 'live'
      ? 'Changes sync live with everyone on this page.'
      : syncStatus === 'offline'
        ? 'Your edits are kept in this tab and sync when the connection is back.'
        : undefined
  const displayTitle = title.trim() ? title : 'Untitled'
  const favorite = favoritesQuery.data?.includes(pageId) ?? false
  const setFavorite = (next: boolean) =>
    toggleFavorite.mutate(
      { pageId, favorite: next },
      { onError: () => toast.error(next ? 'Could not add the page to favorites.' : 'Could not remove the page from favorites.') },
    )
  const favoriteLabel = favorite ? 'Remove from favorites' : 'Add to favorites'

  return (
    <>
      {/* Realtime refreshes may run while the title or editor is focused: remote titles only apply without local
          edits (see the effect above) and content syncs through Yjs, so nothing here is remounted or overwritten. */}
      <section
        data-realtime-safe=""
        className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background max-[899px]:group-data-[view=index]/docs:hidden"
      >
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3 max-[899px]:border-b-0">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="hidden text-muted-foreground/70 max-[899px]:inline-flex"
            aria-label="Back to docs"
            onClick={() => navigate('/docs')}
          >
            <ArrowLeft className="size-4" />
          </Button>
          <nav className="flex min-w-0 items-center gap-1.5 overflow-hidden text-[13px] text-muted-foreground/70" aria-label="Page path">
            <span className="flex min-w-0 shrink-0 items-center gap-1.5" data-space-crumb={space}>
              <span className="flex max-w-40 min-w-0 items-center gap-1">
                {space === PRIVATE_SPACE ? <Lock className="size-3.5 shrink-0" aria-hidden="true" /> : null}
                <span className="truncate">{spaceLabel(space, teamspaces)}</span>
              </span>
              <span className="shrink-0">/</span>
            </span>
            {ancestors.map((ancestor) => (
              <span key={ancestor.id} className="flex min-w-0 items-center gap-1.5">
                <Link
                  className="block min-w-0 truncate rounded-[4px] text-muted-foreground/70 no-underline hover:text-foreground"
                  to={`/docs/${ancestor.id}`}
                >
                  {pageTitle(ancestor)}
                </Link>
                <span className="shrink-0">/</span>
              </span>
            ))}
            <span className="block min-w-0 shrink truncate font-medium text-foreground" aria-current="page">
              {displayTitle}
            </span>
          </nav>
          <span className="flex-1" />
          <PresenceAvatars people={people} />
          <span
            className={cn(
              'flex items-center gap-1.5 whitespace-nowrap text-xs text-muted-foreground/70',
              (status === 'error' || status === 'conflict' || syncStatus === 'lost' || syncStatus === 'error') && 'text-destructive',
            )}
            role="status"
            aria-live="polite"
            title={statusTitle}
            data-save-status={status}
            data-collab-status={syncStatus}
          >
            {status === 'error' || status === 'conflict' ? null : (
              <span className={cn('size-1.5 shrink-0 rounded-full', STATUS_DOT[syncStatus])} aria-hidden="true" />
            )}
            {statusLabel}
            {status === 'error' ? (
              <Button type="button" variant="ghost" size="xs" onClick={() => void saver.flush()}>
                Retry
              </Button>
            ) : null}
          </span>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className={cn('text-muted-foreground/70', favorite && 'text-amber-500 hover:text-amber-500 dark:text-amber-400 dark:hover:text-amber-400')}
            aria-label={favoriteLabel}
            aria-pressed={favorite}
            title={favoriteLabel}
            disabled={favoritesQuery.isPending}
            onClick={() => setFavorite(!favorite)}
          >
            <Star className="size-4" weight={favorite ? 'Filled' : 'Outline'} />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger
              render={<Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Page options" />}
            >
              <Ellipsis className="size-4" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-auto max-w-64 min-w-44">
              {onRequestMove && teamspaces ? (
                <>
                  <DropdownMenuGroup>
                    <DropdownMenuLabel>Move to</DropdownMenuLabel>
                    {spaceTargets.map((target) => (
                      <DropdownMenuItem
                        key={target}
                        className={menuItemClass}
                        disabled={target === space}
                        onClick={() => onRequestMove(pageId, target)}
                      >
                        {target === PRIVATE_SPACE ? <Lock className="size-[14px]" /> : <Users className="size-[14px]" />}
                        <span className="min-w-0 flex-1 truncate">{spaceLabel(target, teamspaces)}</span>
                        {target === space ? <Check className="size-[14px] text-muted-foreground" aria-label="Current space" /> : null}
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuGroup>
                  <DropdownMenuSeparator />
                </>
              ) : null}
              <DropdownMenuItem className={menuItemClass} onClick={() => setFavorite(!favorite)}>
                <Star className="size-[14px]" weight={favorite ? 'Filled' : 'Outline'} />
                {favoriteLabel}
              </DropdownMenuItem>
              <DropdownMenuItem className={menuItemClass} onClick={() => setHistoryOpen(true)}>
                <History className="size-[14px]" />
                Version history
              </DropdownMenuItem>
              {pageActions ? (
                <DropdownMenuItem className={menuItemClass} onClick={() => pageActions.duplicate(pageId, false)}>
                  <Copy className="size-[14px]" />
                  Duplicate
                </DropdownMenuItem>
              ) : null}
              {pageActions && hasChildren ? (
                <DropdownMenuItem className={menuItemClass} onClick={() => pageActions.duplicate(pageId, true)}>
                  <Copy className="size-[14px]" />
                  Duplicate with sub-pages
                </DropdownMenuItem>
              ) : null}
              <DropdownMenuItem className={menuItemClass} data-danger="true" onClick={() => onRequestTrash(pageId)}>
                <Trash2 className="size-[14px]" />
                Move to trash
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        {status === 'conflict' ? (
          <div
            role="alert"
            className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-destructive/10 px-3 py-2 text-[13px] text-foreground"
          >
            <span className="min-w-0 flex-1">The title, icon or cover was changed somewhere else. Your change is not saved.</span>
            <Button type="button" variant="outline" size="sm" onClick={() => void reloadFromServer()}>
              Reload page
            </Button>
            <Button type="button" variant="destructive" size="sm" onClick={overwrite}>
              Overwrite
            </Button>
          </div>
        ) : null}
        {notice ? (
          <div
            role="alert"
            data-sync-notice={notice}
            className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-destructive/10 px-3 py-2 text-[13px] text-foreground"
          >
            <span className="min-w-0 flex-1">
              {notice === 'too-large'
                ? 'Your latest change is too large to sync. Reload the page to continue from the saved version.'
                : notice === 'protocol'
                  ? 'This page cannot sync with this version of Orbit. Reload the app.'
                  : 'Could not reconnect to this page.'}
            </span>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => {
                if (notice === 'protocol') window.location.reload()
                else void reopen()
              }}
            >
              {notice === 'reconnect-failed' ? 'Retry' : notice === 'protocol' ? 'Reload app' : 'Reload page'}
            </Button>
          </div>
        ) : null}
        <div className="min-h-0 flex-1 overflow-y-auto px-6 pt-8 pb-24 max-[899px]:px-3 max-[899px]:pt-[18px] max-[899px]:pb-14">
          {cover.url ? (
            <CoverBanner
              key={`${cover.url}:${cover.position ?? ''}`}
              url={cover.url}
              position={cover.position}
              onChange={changeCover}
              onUpload={uploadCover}
            />
          ) : null}
          <div className="mx-auto max-w-[760px]">
            {icon ? (
              <div className="relative z-[2] w-fit data-[cover=true]:mt-[-52px]" data-cover={cover.url ? 'true' : undefined}>
                <Popover open={iconPickerOpen} onOpenChange={setIconPickerOpen}>
                  <PopoverTrigger
                    render={
                      <Button
                        type="button"
                        variant="ghost"
                        className="h-auto cursor-pointer rounded-xl border-0 p-0.5 text-[60px] leading-none font-normal drop-shadow-[0_1px_2px_rgb(0_0_0/0.3)] hover:bg-foreground/[0.02] dark:hover:bg-foreground/[0.02]"
                        aria-label="Change icon"
                      />
                    }
                  >
                    <Emoji value={icon} size={56} />
                  </PopoverTrigger>
                  <PopoverContent align="start" className={emojiPanelClass}>
                    <EmojiPicker
                      onPick={(emoji) => {
                        changeIcon(emoji)
                        setIconPickerOpen(false)
                      }}
                      onRemove={() => {
                        changeIcon(null)
                        setIconPickerOpen(false)
                      }}
                    />
                  </PopoverContent>
                </Popover>
              </div>
            ) : null}
            {!icon || !cover.url ? (
              <div className="flex gap-2 pt-1.5 pb-2.5">
                {!icon ? (
                  <Popover open={addIconOpen} onOpenChange={setAddIconOpen}>
                    <PopoverTrigger
                      render={<Button type="button" variant="ghost" size="sm" className="text-muted-foreground/70 hover:text-foreground" />}
                    >
                      😀 Add icon
                    </PopoverTrigger>
                    <PopoverContent align="start" className={emojiPanelClass}>
                      <EmojiPicker
                        onPick={(emoji) => {
                          changeIcon(emoji)
                          setAddIconOpen(false)
                        }}
                      />
                    </PopoverContent>
                  </Popover>
                ) : null}
                {!cover.url ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="text-muted-foreground/70 hover:text-foreground"
                    onClick={() => setCoverPanelOpen((open) => !open)}
                  >
                    🖼️ Add cover
                  </Button>
                ) : null}
              </div>
            ) : null}
            {!cover.url && coverPanelOpen ? (
              <div className="mb-3 max-w-[420px] rounded-lg border border-border p-3">
                <CoverSourcePanel
                  onUpload={uploadCover}
                  onPicked={(url) => {
                    setCoverPanelOpen(false)
                    changeCover({ cover_url: url, cover_position: null })
                  }}
                />
              </div>
            ) : null}
            <Input
              className="mb-5 h-auto w-full rounded-none border-0 bg-transparent p-0 text-[30px] leading-[1.25] font-bold text-foreground shadow-none outline-none placeholder:text-muted-foreground/70 focus:outline-none focus-visible:ring-0 md:text-[30px] max-[899px]:mb-3.5 max-[899px]:text-[22px]! dark:bg-transparent"
              value={title}
              maxLength={500}
              placeholder="Untitled"
              aria-label="Page title"
              onChange={(e) => changeTitle(e.target.value)}
              onBlur={() => void saver.flush()}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  editorRef.current?.focus()
                }
              }}
            />
            <div onBlur={() => void saver.flush()}>
              {collab.connection && collab.state.ready ? (
                <Suspense fallback={editorSpinner}>
                  <PageEditor
                    // A new document (page switch, reset) needs a new editor instance.
                    key={collab.key}
                    ref={editorRef}
                    pageId={pageId}
                    collab={{ provider: collab.connection.provider, fragment: collab.connection.fragment, user: collabUser }}
                    editable={!lost && syncStatus !== 'error'}
                    resolvePage={resolvePage}
                    onOpenPage={onOpenPage}
                    onCreateSubpage={onCreateSubpage}
                    onPickPage={onPickPage}
                    uploadFile={uploadFile}
                    className="-mx-[54px] min-h-40 max-[899px]:mx-0"
                  />
                </Suspense>
              ) : (
                // Never bind an editor to a document before its first sync (it would be empty).
                editorSpinner
              )}
            </div>
          </div>
        </div>
        {linkOpen ? (
          <PageLinkDialog pages={pages} excludeId={pageId} onPick={(picked) => closeLinkDialog(picked.id)} onClose={() => closeLinkDialog(null)} />
        ) : null}
      </section>
      {historyOpen ? (
        <PageHistoryPane
          workspaceId={workspaceId}
          pageId={pageId}
          readCurrent={readCurrent}
          resolvePage={resolvePage}
          onOpenPage={onOpenPage}
          onRestore={restoreFromHistory}
          onClose={() => setHistoryOpen(false)}
        />
      ) : null}
    </>
  )
}
