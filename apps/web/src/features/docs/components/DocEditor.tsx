import { lazy, Suspense, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type Ref } from 'react'
import { Link, useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { ArrowLeft, Check, MoreH as Ellipsis, Lock, Star, Trash as Trash2, People as Users } from 'reicon-react'
import { cn } from 'cn'
import { apiClient } from '@/api/client'
import type { Page, PageSummary, Teamspace } from '@/api/generated/types.gen'
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
  isPageVersionConflict,
  pageUploadErrorMessage,
  patchTreePage,
  reconcilePage,
  savePage,
  uploadPageFileRequest,
  useCreatePage,
} from '@/features/docs/api/pages'
import { usePageFavorites, useToggleFavorite } from '@/features/docs/api/favorites'
import { PageAutosaver, saveStatusLabel, type AutosaveState, type PagePatch } from '@/features/docs/autosave'
import { PRIVATE_SPACE, ancestorsOf, pageTitle, spaceKey, spaceLabel, teamspaceSpace, type SpaceKey } from '@/features/docs/pageTree'
import { contentEquals } from '@/features/docs/editor/content'
// Type-only imports: erased at build time, so the BlockNote chunk stays lazy.
import type { PageEditorHandle } from '@/features/docs/editor/PageEditor'
import type { PageRef } from '@/features/docs/editor/pageEditorContext'
import { CoverBanner } from './CoverBanner'
import { CoverSourcePanel } from './CoverSourcePanel'
import { menuItemClass } from './DocTreeItem'
import { PageLinkDialog } from './PageLinkDialog'

// BlockNote + ProseMirror are ~1 MB: load them only when a page is opened.
const PageEditor = lazy(() => import('@/features/docs/editor/PageEditor').then((module) => ({ default: module.PageEditor })))

/** The emoji picker panel brings its own chrome, so the popover is just an anchored frame. */
const emojiPanelClass = 'w-auto gap-0 rounded-xl border border-border bg-popover p-0 text-foreground shadow-xl ring-0'

/** What the docs page needs from the open editor to trash it safely. */
export interface DocEditorControl {
  /** Saves queued edits and resolves with the version to send as `expected_version`. */
  flush: () => Promise<number>
  /** Drops queued edits and stops autosaving (the page is gone). */
  dispose: () => void
}

interface DocEditorProps {
  workspaceId: string
  /** Live page query data: newer versions from realtime refreshes are applied when there are no local edits. */
  page: Page
  pages: PageSummary[]
  /** For the space breadcrumb and the "Move to" menu (undefined while loading). */
  teamspaces?: Teamspace[]
  onRequestTrash: (pageId: string) => void
  /** Moves the page (with its sub-pages) to the end of another space's root. */
  onRequestMove?: (pageId: string, space: SpaceKey) => void
  controlRef?: Ref<DocEditorControl>
}

type Cover = { url: string | null; position: string | null }

function sameEditable(a: Page, b: Page): boolean {
  return (
    a.title === b.title &&
    a.icon === b.icon &&
    a.cover_url === b.cover_url &&
    a.cover_position === b.cover_position &&
    contentEquals(a.content, b.content)
  )
}

export function DocEditor({ workspaceId, page, pages, teamspaces, onRequestTrash, onRequestMove, controlRef }: DocEditorProps) {
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

  const [title, setTitle] = useState(page.title)
  const [icon, setIcon] = useState(page.icon)
  const [cover, setCover] = useState<Cover>({ url: page.cover_url, position: page.cover_position })
  // Content the lazily mounted editor starts from (kept current when a remote update lands before it mounts).
  const [mountContent, setMountContent] = useState<unknown[]>(page.content)
  const [saveState, setSaveState] = useState<AutosaveState>({ status: 'idle', conflict: null, error: null })
  const [coverPanelOpen, setCoverPanelOpen] = useState(false)
  const [iconPickerOpen, setIconPickerOpen] = useState(false)
  const [addIconOpen, setAddIconOpen] = useState(false)
  const [linkOpen, setLinkOpen] = useState(false)
  const pickResolver = useRef<((pageId: string | null) => void) | null>(null)
  /** Last server state the local copy is known to be based on. */
  const baseline = useRef(page)
  const titleRef = useRef(title)

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

  const applyServerPage = useCallback(
    (server: Page) => {
      editorRef.current?.replaceContent(server.content)
      setMountContent(server.content)
      setTitle(server.title)
      titleRef.current = server.title
      setIcon(server.icon)
      setCover({ url: server.cover_url, position: server.cover_position })
      saver.adoptVersion(server.version)
      baseline.current = server
    },
    [saver],
  )

  // Remote updates (realtime refetch, move, another tab): apply them unless there are local edits to protect.
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

  // Flush on unmount (route change, page switch) and when the tab is hidden or closed.
  useEffect(() => {
    const onBeforeUnload = (event: BeforeUnloadEvent) => {
      if (!saver.hasUnsavedChanges()) return
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

  const onContentChange = useCallback((content: unknown[]) => saver.update({ content }), [saver])

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
    const content = editorRef.current?.getContent()
    if (content) local.content = content
    void saver.overwrite(local)
  }

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
  const displayTitle = title.trim() ? title : 'Untitled'
  const favorite = favoritesQuery.data?.includes(pageId) ?? false
  const setFavorite = (next: boolean) =>
    toggleFavorite.mutate(
      { pageId, favorite: next },
      { onError: () => toast.error(next ? 'Could not add the page to favorites.' : 'Could not remove the page from favorites.') },
    )
  const favoriteLabel = favorite ? 'Remove from favorites' : 'Add to favorites'

  return (
    <section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background max-[899px]:group-data-[view=index]/docs:hidden">
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
        <span
          className={cn(
            'flex items-center gap-1.5 whitespace-nowrap text-xs text-muted-foreground/70',
            (status === 'error' || status === 'conflict') && 'text-destructive',
          )}
          role="status"
          aria-live="polite"
          data-save-status={status}
        >
          {saveStatusLabel(saveState, page.updated_at)}
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
          <span className="min-w-0 flex-1">This page was changed somewhere else. Your edits are not saved.</span>
          <Button type="button" variant="outline" size="sm" onClick={() => void reloadFromServer()}>
            Reload page
          </Button>
          <Button type="button" variant="destructive" size="sm" onClick={overwrite}>
            Overwrite
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
            <Suspense
              fallback={
                <div className="flex justify-center py-10">
                  <Spinner className="text-muted-foreground" />
                </div>
              }
            >
              <PageEditor
                ref={editorRef}
                pageId={pageId}
                initialContent={mountContent}
                editable
                onChange={onContentChange}
                resolvePage={resolvePage}
                onOpenPage={onOpenPage}
                onCreateSubpage={onCreateSubpage}
                onPickPage={onPickPage}
                uploadFile={uploadFile}
                className="-mx-[54px] min-h-40 max-[899px]:mx-0"
              />
            </Suspense>
          </div>
        </div>
      </div>
      {linkOpen ? (
        <PageLinkDialog pages={pages} excludeId={pageId} onPick={(picked) => closeLinkDialog(picked.id)} onClose={() => closeLinkDialog(null)} />
      ) : null}
    </section>
  )
}
