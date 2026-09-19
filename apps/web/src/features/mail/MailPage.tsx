import { useState } from 'react'
import { Mail } from 'lucide-react'
import { useParams, useSearchParams } from 'react-router'
import { EmptyState } from '../../components/ui/EmptyState'
import { useAppState } from '../../mock/store'
import { ComposeModal } from './components/ComposeModal'
import { FolderRail } from './components/FolderRail'
import { ThreadList } from './components/ThreadList'
import { ThreadView } from './components/ThreadView'
import { DEFAULT_FOLDER_ID, threadsInFolder } from './mailLib'

export function MailPage() {
  const { threadId } = useParams()
  const [searchParams, setSearchParams] = useSearchParams()
  const state = useAppState()
  const [composeClicked, setComposeClicked] = useState(false)

  // the topbar "New → Email" entry deep-links here with ?compose=1
  const composeOpen = composeClicked || searchParams.get('compose') === '1'
  const setComposeOpen = (open: boolean) => {
    setComposeClicked(open)
    if (!open && searchParams.get('compose')) {
      const next = new URLSearchParams(searchParams)
      next.delete('compose')
      setSearchParams(next, { replace: true })
    }
  }

  const folderId = searchParams.get('folder') ?? DEFAULT_FOLDER_ID
  const folder =
    state.mailFolders.find((f) => f.id === folderId) ??
    state.mailFolders.find((f) => f.id === DEFAULT_FOLDER_ID) ??
    state.mailFolders[0]
  const threads = threadsInFolder(state.mailThreads, folder.id)
  const thread = threadId ? state.mailThreads.find((t) => t.id === threadId) : undefined
  const currentUser = state.users.find((u) => u.id === state.currentUserId)

  return (
    <div
      className="group/mail flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"
      data-view={thread ? 'thread' : 'list'}
    >
      <FolderRail
        folders={state.mailFolders}
        threads={state.mailThreads}
        activeFolderId={folder.id}
      />
      <ThreadList
        folders={state.mailFolders}
        folder={folder}
        threads={threads}
        activeThreadId={thread?.id}
        onCompose={() => setComposeOpen(true)}
      />
      <section className="flex h-full min-h-0 min-w-0 flex-1 flex-col border-l border-border bg-background max-[899px]:border-l-0 max-[899px]:group-data-[view=list]/mail:hidden">
        {thread ? (
          <ThreadView
            key={thread.id}
            thread={thread}
            folders={state.mailFolders}
            activeFolderId={folder.id}
            currentUserEmail={currentUser?.email ?? ''}
          />
        ) : (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <EmptyState
              icon={Mail}
              title="Select a conversation"
              description="Choose a conversation from the list to read it here."
            />
          </div>
        )}
      </section>
      {composeOpen ? <ComposeModal onClose={() => setComposeOpen(false)} /> : null}
    </div>
  )
}
