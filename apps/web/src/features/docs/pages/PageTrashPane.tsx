import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { ArrowLeft, DocumentText as FileText, Lock, Refresh as RotateCcw, Trash as Trash2 } from 'reicon-react'
import type { TrashedPage } from '@/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { confirmAction } from '@/components/common/confirmAction'
import { Emoji } from '@/components/common/Emoji'
import { EmptyState } from '@/components/common/EmptyState'
import { relativeTime } from '@/lib/format'
import {
  canPurgePage,
  isPageVersionConflict,
  useEmptyPageTrash,
  usePageTrash,
  usePurgePage,
  useRestorePage,
} from '@/features/docs/api/pages'
import { useTeamspaces } from '@/features/docs/api/teamspaces'
import { PRIVATE_SPACE, pageTitle, spaceKey, spaceLabel } from '@/features/docs/pageTree'

const ROW = 'flex min-h-10 w-full min-w-0 items-center gap-2.5 border-b border-border px-3 py-1.5'

const plural = (count: number) => `${count} page${count === 1 ? '' : 's'}`

/**
 * Pages trashed directly (their sub-pages come back with them), newest first. "Delete forever" and "Empty trash"
 * cover what `role` may purge: own private pages always, teamspace pages for owners and admins only.
 */
export function PageTrashPane({ workspaceId, role }: { workspaceId: string; role?: string | null }) {
  const navigate = useNavigate()
  const trash = usePageTrash(workspaceId)
  const restore = useRestorePage(workspaceId)
  const purge = usePurgePage(workspaceId)
  const emptyTrash = useEmptyPageTrash(workspaceId)
  const teamspaces = useTeamspaces(workspaceId)
  const purgeable = (trash.data ?? []).filter((page) => canPurgePage(page, role))
  const busy = restore.isPending || purge.isPending || emptyTrash.isPending

  const requestPurge = async (page: TrashedPage) => {
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
        onSuccess: () => toast.success(`Deleted “${pageTitle(page)}” forever`),
        onError: (error) => {
          if (!isPageVersionConflict(error)) toast.error('Could not delete the page.')
        },
      },
    )
  }

  const requestEmpty = async () => {
    const count = purgeable.length
    const kept = (trash.data?.length ?? 0) - count
    const confirmed = await confirmAction({
      title: 'Empty trash?',
      description:
        `${plural(count)} and the sub-pages trashed with them will be deleted forever. This can’t be undone.` +
        (kept > 0 ? ` ${plural(kept)} in teamspaces stay: only workspace owners and admins can delete those.` : ''),
      confirmLabel: 'Empty trash',
      danger: true,
    })
    if (!confirmed) return
    emptyTrash.mutate(undefined, {
      onSuccess: ({ purged }) => toast.success(`Deleted ${plural(purged)} forever`),
      onError: () => toast.error('Could not empty the trash.'),
    })
  }

  let body
  if (trash.isPending) {
    body = (
      <div className="flex flex-1 items-center justify-center">
        <Spinner className="text-muted-foreground" />
      </div>
    )
  } else if (trash.isError) {
    body = (
      <div className="flex flex-1 flex-col p-6">
        <EmptyState
          icon={Trash2}
          title="Trash unavailable"
          description="The server could not load deleted pages."
          action={
            <Button type="button" variant="outline" onClick={() => void trash.refetch()}>
              Retry
            </Button>
          }
        />
      </div>
    )
  } else if (trash.data.length === 0) {
    body = (
      <div className="flex flex-1 flex-col p-6">
        <EmptyState icon={Trash2} title="Trash is empty" description="Pages you move to the trash appear here until restored." />
      </div>
    )
  } else {
    body = (
      <div className="min-h-0 flex-1 overflow-y-auto">
        {trash.data.map((page) => (
          <div className={ROW} key={page.id}>
            <span className="inline-flex w-[18px] shrink-0 items-center justify-center text-muted-foreground/70">
              {page.icon ? <Emoji value={page.icon} size={15} /> : <FileText className="size-[15px]" />}
            </span>
            <span className="min-w-0 flex-1 truncate text-[13px] text-foreground">{pageTitle(page)}</span>
            <span className="flex max-w-40 min-w-0 shrink items-center gap-1 text-xs text-muted-foreground/70" data-space-label>
              {spaceKey(page) === PRIVATE_SPACE ? <Lock className="size-3 shrink-0" aria-hidden="true" /> : null}
              <span className="truncate">{spaceLabel(spaceKey(page), teamspaces.data)}</span>
            </span>
            <span className="shrink-0 text-xs text-muted-foreground/70 max-[899px]:hidden">Deleted {relativeTime(page.deleted_at)}</span>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() =>
                restore.mutate(
                  { pageId: page.id, version: page.version },
                  {
                    onSuccess: (restored) => {
                      toast.success(`Restored “${pageTitle(restored)}”`, {
                        action: { label: 'Open', onClick: () => navigate(`/docs/${restored.id}`) },
                      })
                    },
                    onError: (error) => {
                      if (!isPageVersionConflict(error)) toast.error('Could not restore the page.')
                    },
                  },
                )
              }
            >
              <RotateCcw className="size-3.5" />
              Restore
            </Button>
            {canPurgePage(page, role) ? (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="text-destructive hover:text-destructive"
                disabled={busy}
                aria-label={`Delete “${pageTitle(page)}” forever`}
                onClick={() => void requestPurge(page)}
              >
                <Trash2 className="size-3.5" />
                <span className="max-[899px]:hidden">Delete forever</span>
              </Button>
            ) : null}
          </div>
        ))}
      </div>
    )
  }

  return (
    <section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
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
        <span className="min-w-0 flex-1 truncate text-[13px] font-semibold text-foreground">Trash</span>
        {purgeable.length > 0 ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="text-destructive hover:text-destructive"
            disabled={busy}
            onClick={() => void requestEmpty()}
          >
            Empty trash
          </Button>
        ) : null}
      </div>
      {body}
    </section>
  )
}
