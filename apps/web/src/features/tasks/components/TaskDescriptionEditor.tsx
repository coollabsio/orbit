import { Suspense, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type Ref } from 'react'
import { useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { apiClient, UNAUTHORIZED_EVENT } from '@/api/client'
import { storageQuotaDetail } from '@/api/problem'
import { getTask } from '@/api/generated/sdk.gen'
import { queryKeys } from '@/api/queryKeys'
import { Button } from '@/components/ui/button'
import { useCurrentUser } from '@/features/auth/api'
import { revealCollabCursor } from '@/features/docs/collab/cursor'
import { useCollaborators } from '@/features/docs/collab/presence'
import { PresenceAvatars } from '@/features/docs/collab/PresenceAvatars'
import { collabProblemAction, collabStatusLabel, type CollabProblem, type CollabSession } from '@/features/docs/collab/session'
import { useCollabSession } from '@/features/docs/collab/useCollabSession'
import { pageMentionCandidates } from '@/features/docs/editor/pageMentions'
import { useMembers } from '@/features/workspaces/api'
import type { Task } from '@/features/tasks/api/models'
import { uploadInlineFile } from '@/features/tasks/api/inlineFiles'
import { EditorFallback, PageEditor } from './lazyEditor'
// Type-only imports: erased at build time, so the BlockNote chunk stays lazy.
import type { PageEditorHandle } from '@/features/docs/editor/PageEditor'

export interface TaskDescriptionHandle {
  focus: () => void
}

type Notice = 'too-large' | 'protocol' | null

const editorSpinner = <EditorFallback />

/**
 * The task description, co-edited live like a docs page (`/tasks/{id}/collab`). Until the first sync it shows the
 * stored content read-only; a GitHub-linked task stays read-only (GitHub writes its text).
 */
export function TaskDescriptionEditor({
  workspaceId,
  task,
  readOnly,
  ref,
}: {
  workspaceId: string
  task: Task
  readOnly: boolean
  ref?: Ref<TaskDescriptionHandle>
}) {
  const me = useCurrentUser()
  const selfId = me.data?.id ?? null
  const members = useMembers(workspaceId).data
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const editorRef = useRef<PageEditorHandle>(null)
  useImperativeHandle(ref, () => ({ focus: () => editorRef.current?.focus() }), [])
  // The server stamps every peer's name and color; the color here is a placeholder (see DocEditor).
  const user = useMemo(() => ({ id: me.data?.id, name: me.data?.display_name ?? 'Me', color: '#737373' }), [me.data?.display_name, me.data?.id])
  /** Bumped after a reset: drops the local document and reconnects. */
  const [generation, setGeneration] = useState(0)
  const [notice, setNotice] = useState<Notice>(null)
  /** The task went away while open (trash, access removed): read-only until the task view changes. */
  const [lost, setLost] = useState(false)
  const sessionRef = useRef<CollabSession | null>(null)
  /** A check whether a lost task is back is running (one at a time). */
  const checkingBack = useRef(false)

  const refresh = () => queryClient.invalidateQueries({ queryKey: queryKeys.workspace(workspaceId) })
  const onProblem = (problem: CollabProblem) => {
    switch (collabProblemAction(problem)) {
      case 'leave':
        // The task view shows the trashed or missing task once the refresh lands. A task that is back by then
        // (restored, access returned) reconnects; one restored later opens a fresh editor with its view.
        setLost(true)
        if (checkingBack.current) break
        checkingBack.current = true
        void refresh()
          .then(async () => {
            const { response } = await getTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: task.id } })
            if (!response?.ok) return
            setLost(false)
            setGeneration((current) => current + 1)
          })
          .finally(() => {
            checkingBack.current = false
          })
        break
      case 'unauthorized':
        window.dispatchEvent(new Event(UNAUTHORIZED_EVENT))
        break
      case 'reopen':
        setNotice(null)
        void refresh().then(() => setGeneration((current) => current + 1))
        break
      case 'rate-limited':
        toast.warning('Too many changes at once. Syncing again in a few seconds.', { id: `collab-rate-${task.id}` })
        break
      case 'notice':
        setNotice(problem === 'too-large' ? 'too-large' : 'protocol')
        break
    }
  }
  /** Handshakes keep failing before the upgrade: a REST read tells a trashed task (404) from a network problem. */
  const probe = async () => {
    const { response } = await getTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: task.id } })
    if (response?.status === 404) sessionRef.current?.fail('gone')
  }
  const collab = useCollabSession({
    workspaceId,
    kind: 'tasks',
    id: task.id,
    epoch: task.collabEpoch ?? '',
    generation,
    user,
    onProblem,
    onProbe: () => void probe(),
  })
  useEffect(() => {
    sessionRef.current = collab.session
  }, [collab.session])
  const collaborators = useCollaborators(collab.connection?.provider.awareness ?? null, selfId)
  const people = useMemo(() => {
    const avatars = new Map(members?.map((member) => [member.id, member.avatarUrl]))
    return collaborators.map((person) => ({ ...person, avatarUrl: avatars.get(person.id) }))
  }, [collaborators, members])

  const uploadFile = useCallback(
    async (file: File) => {
      try {
        return await uploadInlineFile(workspaceId, task.id, file)
      } catch (error) {
        toast.error(storageQuotaDetail(error) ?? `Could not upload “${file.name}”.`)
        throw error
      }
    },
    [workspaceId, task.id],
  )
  const onOpenPage = useCallback((pageId: string) => navigate(`/docs/${pageId}`), [navigate])
  const mentions = {
    members: members ?? null,
    // everyone in the workspace sees every task; yourself last
    candidates: pageMentionCandidates(members ?? [], { teamspace_id: workspaceId }, selfId),
    privatePage: false,
  }
  const fileScope = useMemo(() => ({ workspaceId, taskId: task.id }), [workspaceId, task.id])
  const status = collab.state.status
  const editorClass = '-mx-[54px] min-h-16 max-[899px]:mx-0'

  return (
    // Realtime refreshes keep running while the description is focused: its text syncs through Yjs and nothing
    // here is remounted by a refresh.
    <div data-realtime-safe="" className="mt-4">
      {people.length > 0 ? (
        <div className="mb-1 flex justify-end">
          <PresenceAvatars people={people} onReveal={(person) => revealCollabCursor(document, person.id)} />
        </div>
      ) : null}
      {collab.connection && collab.state.ready ? (
        <Suspense fallback={editorSpinner}>
          <PageEditor
            key={collab.key}
            ref={editorRef}
            pageId={`task:${task.id}`}
            profile="task"
            fileScope={fileScope}
            placeholder="description"
            ariaLabel="Description"
            collab={{ provider: collab.connection.provider, fragment: collab.connection.fragment, user }}
            editable={!readOnly && !lost && status !== 'error'}
            onOpenPage={onOpenPage}
            uploadFile={uploadFile}
            mentions={mentions}
            className={editorClass}
          />
        </Suspense>
      ) : (
        // Never bind an editor before the first sync (the document would be empty): show the stored text.
        <div data-testid="description-preview">
          <Suspense fallback={editorSpinner}>
            <PageEditor
              // old markdown links its @mentions once the members are known
              key={`preview:${collab.key}:${members ? 'members' : ''}`}
              pageId={`preview:task:${task.id}`}
              profile="task"
              fileScope={fileScope}
              initialContent={task.descriptionJson ?? undefined}
              initialMarkdown={task.description}
              editable={false}
              onOpenPage={onOpenPage}
              mentions={mentions}
              className={editorClass}
            />
          </Suspense>
        </div>
      )}
      {status === 'offline' || status === 'error' ? (
        <p role="status" className="mt-2 text-xs text-muted-foreground">{collabStatusLabel(collab.state)}</p>
      ) : null}
      {notice ? (
        <p role="alert" className="mt-2 text-xs text-destructive">
          {notice === 'too-large' ? 'Description is too long.' : 'The description could not sync.'}{' '}
          <Button variant="ghost" size="sm" onClick={() => onProblem('reset')}>Reload</Button>
        </p>
      ) : null}
    </div>
  )
}
