import { useState } from 'react'
import { Sms } from 'reicon-react'
import { useParams, useSearchParams } from 'react-router'
import { EmptyState } from '../../components/ui/EmptyState'
import { useAppState } from '../../mock/store'
import { ComposeModal } from './components/ComposeModal'
import { FolderRail } from './components/FolderRail'
import { ThreadList } from './components/ThreadList'
import { ThreadView } from './components/ThreadView'
import { DEFAULT_FOLDER_ID, threadsInFolder } from './mailLib'
import './mail.css'

export function MailPage() {
  const { threadId } = useParams()
  const [searchParams] = useSearchParams()
  const state = useAppState()
  const [composeOpen, setComposeOpen] = useState(false)

  const folderId = searchParams.get('folder') ?? DEFAULT_FOLDER_ID
  const folder =
    state.mailFolders.find((f) => f.id === folderId) ??
    state.mailFolders.find((f) => f.id === DEFAULT_FOLDER_ID) ??
    state.mailFolders[0]
  const threads = threadsInFolder(state.mailThreads, folder.id)
  const thread = threadId ? state.mailThreads.find((t) => t.id === threadId) : undefined
  const currentUser = state.users.find((u) => u.id === state.currentUserId)

  return (
    <div className="page mail-page" data-view={thread ? 'thread' : 'list'}>
      <FolderRail
        folders={state.mailFolders}
        threads={state.mailThreads}
        activeFolderId={folder.id}
        onCompose={() => setComposeOpen(true)}
      />
      <ThreadList
        folders={state.mailFolders}
        folder={folder}
        threads={threads}
        activeThreadId={thread?.id}
        onCompose={() => setComposeOpen(true)}
      />
      <section className="pane mail-view">
        {thread ? (
          <ThreadView
            key={thread.id}
            thread={thread}
            folders={state.mailFolders}
            activeFolderId={folder.id}
            currentUserEmail={currentUser?.email ?? ''}
          />
        ) : (
          <div className="pane-body">
            <EmptyState
              icon={Sms}
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
