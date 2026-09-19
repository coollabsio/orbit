import { useRef, useState } from 'react'
import { Archive, ArrowLeft, Folder, Mail, Star, Trash2 } from 'lucide-react'
import { useNavigate } from 'react-router'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
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
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3 max-[899px]:border-b-0">
        <Button
          variant="ghost"
          size="icon-sm"
          className="hidden text-muted-foreground/70 max-[899px]:inline-flex"
          aria-label="Back"
          onClick={() => navigate(listUrl)}
        >
          <ArrowLeft className="size-4" />
        </Button>
        <span className="min-w-0 shrink truncate text-[13px] font-semibold text-foreground">{thread.subject}</span>
        <span className="flex-1" />
        <Button
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground/70"
          aria-label={thread.starred ? 'Unstar' : 'Star'}
          onClick={() => toggleThreadStar(thread.id)}
        >
          <Star className={cn('size-4', thread.starred && 'fill-[#fcd452] text-[#fcd452]')} />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground/70"
          aria-label="Mark as unread"
          onClick={() => {
            setThreadRead(thread.id, false)
            navigate(listUrl)
          }}
        >
          <Mail className="size-4" />
        </Button>
        <Dropdown
          align="right"
          trigger={() => (
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Move conversation">
              <Folder className="size-4" />
            </Button>
          )}
        >
          {(close) => (
            <div className="p-1">
              <span className="block px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase">
                Move to
              </span>
              {folders
                .filter((candidate) => candidate.id !== 'f_starred' && candidate.id !== thread.folderId)
                .map((candidate) => {
                  const Icon = FOLDER_ICONS[candidate.icon]
                  return (
                    <button
                      key={candidate.id}
                      type="button"
                      className="flex min-h-8 w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-accent"
                      onClick={() => {
                        close()
                        moveTo(candidate.id)
                      }}
                    >
                      <Icon className="size-3.5" />
                      {candidate.name}
                    </button>
                  )
                })}
            </div>
          )}
        </Dropdown>
        <Button
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground/70"
          aria-label="Archive"
          onClick={() => {
            moveTo('f_archive')
          }}
        >
          <Archive className="size-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground/70"
          aria-label="Move to trash"
          onClick={() => {
            moveTo('f_trash')
          }}
        >
          <Trash2 className="size-4" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex max-w-[800px] flex-col px-5 pt-6 pb-8">
          <div className="flex flex-wrap items-center gap-2.5 pb-2">
            <h1 className="m-0 text-xl font-semibold text-foreground">{thread.subject}</h1>
            {folder ? (
              <span className="inline-flex items-center gap-1 rounded-full bg-muted px-2 py-0.5 text-[10px] leading-[14px] font-medium text-muted-foreground">
                {folder.name}
              </span>
            ) : null}
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
