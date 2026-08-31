import { useRef, useState } from 'react'
import { Archive, ArrowLeft, Folder, Sms, Star, Trash } from 'reicon-react'
import { useNavigate } from 'react-router'
import { Dropdown } from '../../../components/ui/Dropdown'
import { moveThread, setThreadRead, toggleThreadStar } from '../../../mock/actions'
import type { MailFolder, MailThread } from '../../../mock/types'
import { FOLDER_ICONS, threadSender } from '../mailLib'
import { MessageItem } from './MessageItem'
import { ReplyComposer } from './ReplyComposer'
import { ComposeModal } from './ComposeModal'

interface ThreadViewProps {
  thread: MailThread
  folders: MailFolder[]
  activeFolderId: string
  currentUserEmail: string
}

export function ThreadView({ thread, folders, activeFolderId, currentUserEmail }: ThreadViewProps) {
  const navigate = useNavigate()
  const [expandedOverrides, setExpandedOverrides] = useState<Record<string, boolean>>({})
  const [forwarding, setForwarding] = useState<MailThread['messages'][number] | null>(null)
  const [replying, setReplying] = useState(false)
  const replyRef = useRef<HTMLTextAreaElement>(null)
  const listUrl = `/mail?folder=${activeFolderId}`
  const folder = folders.find((f) => f.id === thread.folderId)
  const lastIndex = thread.messages.length - 1

  const toggleMessage = (id: string, fallback: boolean) => {
    setExpandedOverrides((prev) => ({ ...prev, [id]: !(prev[id] ?? fallback) }))
  }

  const moveTo = (folderId: string) => {
    moveThread(thread.id, folderId)
    navigate(`/mail?folder=${activeFolderId}`)
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
        <Dropdown
          align="right"
          trigger={() => (
            <button className="icon-button" aria-label="Move conversation">
              <Folder size={16} />
            </button>
          )}
        >
          {(close) => (
            <>
              <span className="popover-label">Move to</span>
              {folders
                .filter((candidate) => candidate.id !== 'f_starred' && candidate.id !== thread.folderId)
                .map((candidate) => {
                  const Icon = FOLDER_ICONS[candidate.icon]
                  return (
                    <button
                      key={candidate.id}
                      type="button"
                      className="popover-option"
                      onClick={() => {
                        close()
                        moveTo(candidate.id)
                      }}
                    >
                      <Icon size={14} />
                      {candidate.name}
                    </button>
                  )
                })}
            </>
          )}
        </Dropdown>
        <button
          className="icon-button"
          aria-label="Archive"
          onClick={() => {
            moveTo('f_archive')
          }}
        >
          <Archive size={16} />
        </button>
        <button
          className="icon-button"
          aria-label="Move to trash"
          onClick={() => {
            moveTo('f_trash')
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
                onReply={() => {
                  setReplying(true)
                  requestAnimationFrame(() => {
                    replyRef.current?.focus()
                    replyRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' })
                  })
                }}
                onForward={() => setForwarding(message)}
              />
            )
          })}
          {replying ? (
            <ReplyComposer
              threadId={thread.id}
              replyToName={
                [...thread.messages].reverse().find((m) => m.from.email !== currentUserEmail)?.from
                  .name ?? threadSender(thread).name
              }
              textareaRef={replyRef}
              onClose={() => setReplying(false)}
            />
          ) : null}
        </div>
      </div>
      {forwarding ? (
        <ComposeModal
          onClose={() => setForwarding(null)}
          initial={{
            subject: thread.subject.startsWith('Fwd:') ? thread.subject : `Fwd: ${thread.subject}`,
            body: `\n\n---------- Forwarded message ----------\nFrom: ${forwarding.from.name} <${forwarding.from.email}>\nDate: ${new Date(forwarding.createdAt).toLocaleString()}\nSubject: ${thread.subject}\n\n${forwarding.body}`,
          }}
        />
      ) : null}
    </>
  )
}
