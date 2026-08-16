import { Edit } from 'reicon-react'
import { useNavigate } from 'react-router'
import { EmptyState } from '../../../components/ui/EmptyState'
import { setThreadRead } from '../../../mock/actions'
import type { MailFolder, MailThread } from '../../../mock/types'
import { FOLDER_ICONS } from '../mailLib'
import { ThreadRow } from './ThreadRow'

interface ThreadListProps {
  folders: MailFolder[]
  folder: MailFolder
  threads: MailThread[]
  activeThreadId: string | undefined
  onCompose: () => void
}

export function ThreadList({ folders, folder, threads, activeThreadId, onCompose }: ThreadListProps) {
  const navigate = useNavigate()
  const FolderIcon = FOLDER_ICONS[folder.icon]

  const openThread = (threadId: string) => {
    setThreadRead(threadId, true)
    navigate(`/mail/${threadId}?folder=${folder.id}`)
  }

  return (
    <section className="pane mail-list">
      <div className="pane-header">
        <span className="pane-title mail-desktop">{folder.name}</span>
        <span className="pane-title mail-mobile">Mail</span>
        <span className="spacer" />
        <button className="button button-primary mail-desktop" onClick={onCompose}>
          <Edit size={16} />
          Compose
        </button>
        <button className="icon-button mail-mobile" aria-label="Compose" onClick={onCompose}>
          <Edit size={16} />
        </button>
      </div>
      <div className="pane-toolbar mail-mobile">
        {folders.map((f) => (
          <button
            key={f.id}
            className="app-tab"
            data-active={f.id === folder.id || undefined}
            onClick={() => navigate(`/mail?folder=${f.id}`)}
          >
            {f.name}
          </button>
        ))}
      </div>
      <div className="pane-body">
        {threads.length === 0 ? (
          <EmptyState
            icon={FolderIcon}
            title="Nothing here"
            description={`No conversations in ${folder.name}.`}
          />
        ) : (
          threads.map((thread) => (
            <ThreadRow
              key={thread.id}
              thread={thread}
              active={thread.id === activeThreadId}
              onOpen={openThread}
            />
          ))
        )}
      </div>
    </section>
  )
}
