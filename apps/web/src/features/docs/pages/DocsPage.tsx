import { useEffect, useRef } from 'react'
import { Link, Navigate, useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'
import { DocumentText as FileText, Add as Plus } from 'reicon-react'
import { Button, buttonVariants } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { EmptyState } from '@/components/common/EmptyState'
import { confirmAction } from '@/components/common/confirmAction'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { isPageNotFound, isPageVersionConflict, useCreatePage, useMovePage, usePage, usePageTree, useTrashPage } from '@/features/docs/api/pages'
import { canDeleteTeamspaces, useTeamspaces } from '@/features/docs/api/teamspaces'
import { descendantsOf, dropOnSpace, landingPage, pageTitle, spaceKey, spaceLabel, type SpaceKey } from '@/features/docs/pageTree'
import { DocEditor, type DocEditorControl } from '@/features/docs/components/DocEditor'
import { DocTree } from '@/features/docs/components/DocTree'
import { NotionImportPane } from './NotionImportPane'
import { PageTrashPane } from './PageTrashPane'

const PANE = 'flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background max-[899px]:group-data-[view=index]/docs:hidden'

/** Desktop lands on the first page; phones show the page list first (the tree is the whole screen there). */
function landsOnFirstPage() {
  return typeof window.matchMedia !== 'function' || window.matchMedia('(min-width: 900px)').matches
}

export function DocsPage({ view = 'page' }: { view?: 'page' | 'trash' | 'import' }) {
  const { pageId, importId } = useParams()
  const navigate = useNavigate()
  const { workspace } = useWorkspace()
  const tree = usePageTree(workspace.id)
  const teamspaces = useTeamspaces(workspace.id)
  const page = usePage(workspace.id, view === 'page' ? pageId : undefined)
  const createPage = useCreatePage(workspace.id)
  const trashPage = useTrashPage(workspace.id)
  const movePage = useMovePage(workspace.id)
  const editorControl = useRef<DocEditorControl>(null)
  const pages = tree.data ?? []
  const activeId = view === 'page' ? (pageId ?? null) : null
  // The trash request is async: read the open page when it finishes, not when it started.
  const activeIdRef = useRef(activeId)
  useEffect(() => {
    activeIdRef.current = activeId
  })

  const createFirstPage = () => {
    createPage.mutate(
      {},
      {
        onSuccess: (created) => navigate(`/docs/${created.id}`),
        onError: () => toast.error('Could not create the page. Try again.'),
      },
    )
  }

  /** Confirm, save the open editor if it is affected, trash the subtree, then leave it if it was open. */
  const requestTrash = async (targetId: string) => {
    const target = pages.find((item) => item.id === targetId)
    if (!target) return
    const descendants = descendantsOf(pages, targetId)
    const confirmed = await confirmAction({
      title: `Move “${pageTitle(target)}” to the trash?`,
      description:
        descendants.length > 0
          ? `Its ${descendants.length} sub-page${descendants.length === 1 ? '' : 's'} move with it. You can restore them from Trash.`
          : 'You can restore it from Trash.',
      confirmLabel: 'Move to trash',
      danger: true,
    })
    if (!confirmed) return

    const affected = new Set([targetId, ...descendants.map((item) => item.id)])
    const isAffected = (id: string | null) => id !== null && affected.has(id)
    // Capture the affected editor now: by the time the request finishes another page may be open.
    const control = isAffected(activeIdRef.current) ? editorControl.current : null
    let version = target.version
    if (control) {
      const saved = await control.flush()
      if (activeIdRef.current === targetId) version = saved
    }
    trashPage.mutate(
      { pageId: targetId, version },
      {
        onSuccess: () => {
          control?.dispose()
          if (isAffected(activeIdRef.current)) {
            navigate(target.parent_id ? `/docs/${target.parent_id}` : '/docs', { replace: true })
          }
          toast.success('Moved to trash')
        },
        onError: (error) => {
          if (!isPageVersionConflict(error)) toast.error('Could not move the page to the trash.')
        },
      },
    )
  }

  /** Page menu "Move to": the page (and its sub-pages) becomes the last root page of `space`. */
  const requestMoveToSpace = async (targetId: string, space: SpaceKey) => {
    const target = pages.find((item) => item.id === targetId)
    const move = target ? dropOnSpace(pages, targetId, space) : null
    if (!target || !move) return
    // Save pending edits first so the move is sent on the page's latest version.
    let version = target.version
    if (activeIdRef.current === targetId && editorControl.current) version = await editorControl.current.flush()
    movePage.mutate(
      { pageId: targetId, version, ...move },
      {
        onSuccess: () => {
          if (spaceKey(target) !== space) toast.success(`Moved to ${spaceLabel(space, teamspaces.data)}`)
        },
        onError: (error) => {
          if (!isPageVersionConflict(error)) toast.error('Could not move the page.')
        },
      },
    )
  }

  let content
  if (view === 'trash') {
    content = <PageTrashPane workspaceId={workspace.id} />
  } else if (view === 'import') {
    content = <NotionImportPane key={importId ?? 'new'} workspaceId={workspace.id} importId={importId} />
  } else if (!pageId) {
    // The landing order needs the default teamspace; a failed teamspace list falls back to private / any page.
    const spacesKnown = !teamspaces.isPending
    const first = spacesKnown ? landingPage(pages, teamspaces.data) : null
    if (first && landsOnFirstPage()) return <Navigate to={`/docs/${first.id}`} replace />
    content = (
      <section className={PANE}>
        {tree.isPending || !spacesKnown ? (
          <div className="flex flex-1 items-center justify-center">
            <Spinner className="text-muted-foreground" />
          </div>
        ) : (
          <div className="flex flex-1 flex-col p-6">
            {first ? (
              <EmptyState icon={FileText} title="Select a page" description="Choose a page from the list or create a new one." />
            ) : (
              <EmptyState
                icon={FileText}
                title="Create your first page"
                description="New pages go to your team's default teamspace. Private pages are only visible to you."
                action={
                  <Button type="button" disabled={createPage.isPending} onClick={createFirstPage}>
                    <Plus className="size-4" />
                    Create page
                  </Button>
                }
              />
            )}
          </div>
        )}
      </section>
    )
  } else if (page.data) {
    content = (
      <DocEditor
        key={page.data.id}
        workspaceId={workspace.id}
        page={page.data}
        pages={pages}
        teamspaces={teamspaces.data}
        controlRef={editorControl}
        onRequestTrash={(id) => void requestTrash(id)}
        onRequestMove={(id, space) => void requestMoveToSpace(id, space)}
      />
    )
  } else if (page.isError) {
    const missing = isPageNotFound(page.error)
    content = (
      <section className={PANE}>
        <div className="flex flex-1 flex-col p-6">
          <EmptyState
            icon={FileText}
            title={missing ? 'Page not found' : 'Page unavailable'}
            description={missing ? 'This page was moved to the trash or does not exist.' : 'The server could not load this page.'}
            action={
              <div className="flex gap-2">
                {missing ? null : (
                  <Button type="button" variant="outline" onClick={() => void page.refetch()}>
                    Retry
                  </Button>
                )}
                <Link className={buttonVariants({ variant: 'outline' })} to="/docs">
                  Back to Docs
                </Link>
              </div>
            }
          />
        </div>
      </section>
    )
  } else {
    content = (
      <section className={PANE}>
        <div className="flex flex-1 items-center justify-center">
          <Spinner className="text-muted-foreground" />
        </div>
      </section>
    )
  }

  return (
    <div
      className="group/docs flex min-h-0 min-w-0 flex-1 overflow-hidden bg-card"
      data-view={view !== 'page' || pageId ? 'doc' : 'index'}
    >
      <DocTree
        key={workspace.id}
        workspaceId={workspace.id}
        pages={tree.data}
        isPending={tree.isPending}
        isError={tree.isError}
        onRetry={() => void tree.refetch()}
        activeId={activeId}
        trashActive={view === 'trash'}
        canDeleteTeamspaces={canDeleteTeamspaces(workspace.role)}
        onTrash={(id) => void requestTrash(id)}
      />
      {content}
    </div>
  )
}
