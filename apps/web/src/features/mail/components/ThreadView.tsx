import { useState } from 'react'
import { Archive, ArrowLeft, Sms, Star, Trash } from 'reicon-react'
import { useNavigate } from 'react-router'
import { moveThread, setThreadRead, toggleThreadStar } from '../../../mock/actions'
import type { MailFolder, MailThread } from '../../../mock/types'
import { threadSender } from '../mailLib'
import { MessageItem } from './MessageItem'
import { ReplyComposer } from './ReplyComposer'

interface ThreadViewProps {
  thread: MailThread
  folders: MailFolder[]
  activeFolderId: string
  currentUserEmail: string
}

export function ThreadView({ thread, folders, activeFolderId, currentUserEmail }: ThreadViewProps) {
  const navigate = useNavigate()
  const [expandedOverrides, setExpandedOverrides] = useState<Record<string, boolean>>({})
  const listUrl = `/mail?folder=${activeFolderId}`
  const folder = folders.find((f) => f.id === thread.folderId)
  const lastIndex = thread.messages.length - 1

  const toggleMessage = (id: string, fallback: boolean) => {
    setExpandedOverrides((prev) => ({ ...prev, [id]: !(prev[id] ?? fallback) }))
  }

  return (
    <>
      <div className="pane-header">
        <button
          className="icon-button mail-mobile"
          aria-label="Back"
          onClick={() => navigate(listUrl)}
        >
          <ArrowLeft size={16} />
        </button>
        <span className="pane-title truncate mail-view-title">{thread.subject}</span>
        <span className="spacer" />
        <button
          className="icon-button"
          aria-label={thread.starred ? 'Unstar' : 'Star'}
          onClick={() => toggleThreadStar(thread.id)}
        >
          <Star
            size={16}
            weight={thread.starred ? 'Filled' : 'Outline'}
            color={thread.starred ? 'var(--warning-dot)' : undefined}
          />
        </button>
        <button
          className="icon-button"
          aria-label="Mark as unread"
          onClick={() => {
            setThreadRead(thread.id, false)
            navigate(listUrl)
          }}
        >
          <Sms size={16} />
        </button>
        <button
          className="icon-button"
          aria-label="Archive"
          onClick={() => {
            moveThread(thread.id, 'f_archive')
            navigate(listUrl)
          }}
        >
          <Archive size={16} />
        </button>
        <button
          className="icon-button"
          aria-label="Move to trash"
          onClick={() => {
            moveThread(thread.id, 'f_trash')
            navigate(listUrl)
          }}
        >
          <Trash size={16} />
        </button>
      </div>
      <div className="pane-body">
        <div className="mail-thread">
          <div className="mail-thread-heading">
            <h1 className="mail-thread-subject">{thread.subject}</h1>
            {folder ? <span className="badge">{folder.name}</span> : null}
          </div>
          {thread.messages.map((message, index) => {
            const defaultExpanded = index === lastIndex
            return (
              <MessageItem
                key={message.id}
                message={message}
                expanded={expandedOverrides[message.id] ?? defaultExpanded}
                onToggle={() => toggleMessage(message.id, defaultExpanded)}
              />
            )
          })}
          <ReplyComposer
            threadId={thread.id}
            replyToName={
              [...thread.messages].reverse().find((m) => m.from.email !== currentUserEmail)?.from
                .name ?? threadSender(thread).name
            }
          />
        </div>
      </div>
    </>
  )
}
