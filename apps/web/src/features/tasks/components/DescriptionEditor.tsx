import { lazy, Suspense, useCallback, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { apiClient, UNAUTHORIZED_EVENT } from '@/api/client'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { useCurrentUser } from '@/features/auth/api'
import { pageUploadErrorMessage, uploadPageFileRequest, usePage } from '@/features/docs/api/pages'
import { collabColor } from '@/features/docs/collab/palette'
import type { CollabProblem } from '@/features/docs/collab/session'
import { useCollabSession } from '@/features/docs/collab/useCollabSession'

const PageEditor = lazy(() => import('@/features/docs/editor/PageEditor').then((module) => ({ default: module.PageEditor })))

// A description stands alone: it has no sub-pages and links to no Docs page.
const noPage = () => null
const noOpen = () => {}
const noSubpage = () => Promise.reject(new Error('A description has no sub-pages.'))
const noPick = () => Promise.resolve(null)

const spinner = (
  <div className="flex justify-center py-6">
    <Spinner className="text-muted-foreground" />
  </div>
)

/**
 * The description of a project or a milestone: the Docs editor on a hidden page that the owner holds. The page is
 * made on the first use (`onCreate`), so an owner with no description costs no page.
 */
export function DescriptionEditor({ workspaceId, pageId, onCreate }: {
  workspaceId: string
  /** Null until someone writes a description. */
  pageId: string | null
  /** Creates the page and resolves to its id. */
  onCreate: () => Promise<string>
}) {
  const [createdId, setCreatedId] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const id = pageId ?? createdId

  if (!id) {
    return (
      <Button
        variant="ghost"
        className="-ml-2 justify-start font-normal text-muted-foreground"
        disabled={creating}
        onClick={() => {
          setCreating(true)
          onCreate().then(setCreatedId, () => toast.error('Could not open the description.')).finally(() => setCreating(false))
        }}
      >
        {creating ? 'Opening…' : 'Add a description…'}
      </Button>
    )
  }
  return <OwnedPageEditor key={id} workspaceId={workspaceId} pageId={id} />
}

function OwnedPageEditor({ workspaceId, pageId }: { workspaceId: string; pageId: string }) {
  const page = usePage(workspaceId, pageId)
  if (page.isError) return <p role="alert" className="text-xs text-destructive">The description could not be loaded.</p>
  if (!page.data) return spinner
  return <LiveEditor workspaceId={workspaceId} pageId={pageId} epoch={page.data.collab_epoch} refetch={() => page.refetch()} />
}

function LiveEditor({ workspaceId, pageId, epoch, refetch }: { workspaceId: string; pageId: string; epoch: string; refetch: () => Promise<unknown> }) {
  const me = useCurrentUser()
  const user = useMemo(
    () => ({ name: me.data?.display_name ?? 'Me', color: collabColor(me.data?.id ?? '') }),
    [me.data?.display_name, me.data?.id],
  )
  /** Bumped after the server reset the document: drops the local document and connects again. */
  const [generation, setGeneration] = useState(0)
  /** The page went away while open (the owner was deleted): the editor turns read-only. */
  const [lost, setLost] = useState(false)

  const onProblem = (problem: CollabProblem) => {
    switch (problem) {
      case 'gone':
      case 'forbidden':
        setLost(true)
        break
      case 'session':
        window.dispatchEvent(new Event(UNAUTHORIZED_EVENT))
        break
      case 'reset':
        void refetch().then(() => setGeneration((current) => current + 1))
        break
      case 'rate-limited':
        toast.warning('Too many changes at once. Syncing again in a few seconds.', { id: `collab-rate-${pageId}` })
        break
      case 'too-large':
        toast.error('The description is too large to save.')
        break
      case 'protocol':
        toast.error('The description could not sync. Reload the page.')
        break
    }
  }

  const collab = useCollabSession({ workspaceId, pageId, epoch, generation, user, onProblem, onProbe: () => void refetch() })

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

  if (!collab.connection || !collab.state.ready) return spinner
  return (
    // the Y document merges remote edits itself, so realtime refreshes need not wait for this field to lose focus
    <div data-realtime-safe="" data-slot="description-editor" className="min-h-24">
      <Suspense fallback={spinner}>
        <PageEditor
          key={collab.key}
          pageId={pageId}
          collab={{ provider: collab.connection.provider, fragment: collab.connection.fragment, user }}
          editable={!lost && collab.state.status !== 'error'}
          resolvePage={noPage}
          onOpenPage={noOpen}
          onCreateSubpage={noSubpage}
          onPickPage={noPick}
          uploadFile={uploadFile}
        />
      </Suspense>
    </div>
  )
}
