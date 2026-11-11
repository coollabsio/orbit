import { lazy, Suspense, useCallback } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { ArrowLeft, DocumentText as FileText, Refresh as RotateCcw, Trash as Trash2 } from 'reicon-react'
import type { PageSummary } from '@orbit/api/generated/types.gen'
import { Button, buttonVariants } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { confirmAction } from '@/components/common/confirmAction'
import { Emoji } from '@/components/common/Emoji'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { shortDate } from '@/lib/format'
import { useMembers } from '@/features/workspaces/api'
import { isPageNotFound, isPageVersionConflict, usePageTree, usePurgePage, useRestorePage, useTrashedPage } from '@/features/docs/api/pages'
import { CoverBanner } from '@/features/docs/components/CoverBanner'
// Type-only: erased at build time, so the BlockNote chunk stays lazy.
import type { PageRef } from '@/features/docs/editor/pageEditorContext'
import { childrenOf, pageTitle } from '@/features/docs/pageTree'

const PageEditor = lazy(() => import('@/features/docs/editor/PageEditor').then((module) => ({ default: module.PageEditor })))

const DAY = 24 * 60 * 60 * 1000
const RETENTION_DAYS = 30

const noop = () => {}
const noSubpage = () => Promise.reject(new Error('Read-only preview'))
const noPick = () => Promise.resolve(null)

/** Whole days until the trash deletes the page for good (rounded up, never below 0). */
function daysLeft(deletedAt: string, now = Date.now()): number {
  return Math.max(0, Math.ceil((new Date(deletedAt).getTime() + RETENTION_DAYS * DAY - now) / DAY))
}

/** The sub-pages under `parentId` in tree order, each with its depth below the previewed page. */
function nested(pages: readonly PageSummary[], parentId: string, depth = 0): { page: PageSummary; depth: number }[] {
  return childrenOf(pages, parentId).flatMap((page) => [{ page, depth }, ...nested(pages, page.id, depth + 1)])
}

/**
 * A trashed page, read-only: a banner with who trashed it, when the trash deletes it, "Restore page" and (for
 * `can_purge`) "Delete forever", then its cover, icon, title, content and the sub-pages trashed with it. No co-editing
 * socket: the server closes it for trashed pages.
 */
export function PageTrashPreview({ workspaceId, pageId }: { workspaceId: string; pageId: string }) {
  const navigate = useNavigate()
  const detail = useTrashedPage(workspaceId, pageId)
  const members = useMembers(workspaceId)
  const tree = usePageTree(workspaceId)
  const restore = useRestorePage(workspaceId)
  const purge = usePurgePage(workspaceId)
  const busy = restore.isPending || purge.isPending
  const subPages = detail.data?.sub_pages

  const resolvePage = useCallback(
    (id: string): PageRef | null => {
      const found = subPages?.find((page) => page.id === id) ?? tree.data?.find((page) => page.id === id)
      return found ? { title: found.title, icon: found.icon, trashed: false } : null
    },
    [subPages, tree.data],
  )
  const onOpenPage = useCallback(
    (id: string) => navigate(subPages?.some((page) => page.id === id) ? `/docs/trash/${id}` : `/docs/${id}`),
    [navigate, subPages],
  )

  const back = (
    <PaneHeader>
      <Link to="/docs/trash" aria-label="Back to the trash" className={buttonVariants({ variant: 'ghost', size: 'icon-sm', className: 'text-muted-foreground/70' })}>
        <ArrowLeft className="size-4" />
      </Link>
      <PaneTitle className="min-w-0 flex-1">Trash</PaneTitle>
    </PaneHeader>
  )

  if (!detail.data) {
    return (
      <Pane>
        {back}
        {detail.isError ? (
          <div className="flex flex-1 flex-col p-6">
            <EmptyState
              icon={Trash2}
              title={isPageNotFound(detail.error) ? 'Page not in the trash' : 'Page unavailable'}
              description={
                isPageNotFound(detail.error)
                  ? 'It was restored or deleted forever, or its 30 days in the trash are over.'
                  : 'The server could not load this page.'
              }
              action={
                isPageNotFound(detail.error) ? undefined : (
                  <Button type="button" variant="outline" onClick={() => void detail.refetch()}>
                    Retry
                  </Button>
                )
              }
            />
          </div>
        ) : (
          <div className="flex flex-1 items-center justify-center">
            <Spinner className="text-muted-foreground" />
          </div>
        )}
      </Pane>
    )
  }

  const { page, can_purge: canPurge, deleted_by: deletedBy } = detail.data
  const who = members.data?.find((member) => member.id === deletedBy)?.name ?? 'Someone'
  const days = daysLeft(page.deleted_at ?? page.updated_at)
  const when = page.deleted_at ? ` ${shortDate(page.deleted_at)}` : ''
  const children = nested(detail.data.sub_pages, page.id)

  const requestRestore = () =>
    restore.mutate(
      { pageId: page.id, version: page.version },
      {
        onSuccess: (restored) => {
          toast.success(`Restored “${pageTitle(restored)}”`)
          navigate(`/docs/${restored.id}`, { replace: true })
        },
        onError: (error) => {
          if (!isPageVersionConflict(error)) toast.error('Could not restore the page.')
        },
      },
    )

  const requestPurge = async () => {
    const confirmed = await confirmAction({
      title: `Delete “${pageTitle(page)}” forever?`,
      description: 'The page, the sub-pages trashed with it and their files are deleted permanently. This can’t be undone.',
      confirmLabel: 'Delete forever',
      danger: true,
    })
    if (!confirmed) return
    purge.mutate(
      { pageId: page.id, version: page.version },
      {
        onSuccess: () => {
          toast.success(`Deleted “${pageTitle(page)}” forever`)
          navigate('/docs/trash', { replace: true })
        },
        onError: (error) => {
          if (!isPageVersionConflict(error)) toast.error('Could not delete the page.')
        },
      },
    )
  }

  return (
    <Pane>
      {back}
      <div role="alert" className="flex shrink-0 flex-wrap items-center justify-center gap-x-4 gap-y-2 bg-destructive px-4 py-2 text-[0.8125rem] text-white">
        <span className="min-w-0 text-center">
          {who} moved this page to Trash{when}. It will be permanently deleted {days === 0 ? 'today' : `in ${days} day${days === 1 ? '' : 's'}`}.
        </span>
        <div className="flex shrink-0 gap-2">
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="border-white/70 bg-transparent text-white hover:bg-white/15 hover:text-white dark:border-white/70 dark:bg-transparent dark:hover:bg-white/15"
            disabled={busy}
            onClick={requestRestore}
          >
            {restore.isPending ? <Spinner /> : <RotateCcw className="size-3.5" />}
            Restore page
          </Button>
          {canPurge ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="border-white/70 bg-transparent text-white hover:bg-white/15 hover:text-white dark:border-white/70 dark:bg-transparent dark:hover:bg-white/15"
              disabled={busy}
              onClick={() => void requestPurge()}
            >
              {purge.isPending ? <Spinner /> : <Trash2 className="size-3.5" />}
              Delete forever
            </Button>
          ) : null}
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto pb-16" aria-label="Page preview" role="region">
        {page.cover_url ? <CoverBanner url={page.cover_url} position={page.cover_position} onChange={noop} readOnly /> : null}
        <div className="mx-auto max-w-[708px] px-6 pt-10 max-[899px]:px-3 max-[899px]:pt-6">
          {page.icon ? (
            <div className="mb-2 text-[3.5rem] leading-none">
              <Emoji value={page.icon} />
            </div>
          ) : null}
          <h1 className="mb-4 text-[2rem] leading-tight font-bold text-foreground">{pageTitle(page)}</h1>
          <Suspense
            fallback={
              <div className="flex justify-center py-10">
                <Spinner className="text-muted-foreground" />
              </div>
            }
          >
            <PageEditor
              key={page.id}
              pageId={`trash:${page.id}`}
              initialContent={page.content}
              editable={false}
              onChange={noop}
              resolvePage={resolvePage}
              onOpenPage={onOpenPage}
              onCreateSubpage={noSubpage}
              onPickPage={noPick}
              className="-mx-[54px] max-[899px]:mx-0"
            />
          </Suspense>
          {children.length > 0 ? (
            <section aria-label="Sub-pages in the trash" className="mt-8 border-t border-border pt-4">
              <h2 className="mb-1 text-xs font-medium text-muted-foreground">Sub-pages trashed with it</h2>
              {children.map(({ page: child, depth }) => (
                <Link
                  key={child.id}
                  to={`/docs/trash/${child.id}`}
                  className="flex min-h-8 items-center gap-2 rounded-md px-1.5 text-[0.8125rem] text-foreground hover:bg-accent/60"
                  style={{ paddingLeft: 6 + depth * 18 }}
                >
                  <span className="inline-flex w-[18px] shrink-0 items-center justify-center text-muted-foreground/70">
                    {child.icon ? <Emoji value={child.icon} /> : <FileText className="size-[15px]" />}
                  </span>
                  <span className="truncate">{pageTitle(child)}</span>
                </Link>
              ))}
            </section>
          ) : null}
        </div>
      </div>
    </Pane>
  )
}
