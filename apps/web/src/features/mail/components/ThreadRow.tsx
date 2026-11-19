import type { ReactNode } from 'react'
import { Archive, DirectInbox, Paperclip2 as Paperclip, Sms as Mail, SmsTracking, Star, Trash } from 'reicon-react'
import { cn } from 'cn'
import type { MailLabel, MailThreadSummary } from '@orbit/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { Tip } from '@/components/common/Tip'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { relativeTime } from '@/lib/format'
import { personName, sentRecipients, threadSenders } from '@/features/mail/mailLib'
import type { AvatarOf } from './useMailAvatar'

interface ThreadRowProps {
  thread: MailThreadSummary
  labels: ReadonlyMap<string, MailLabel>
  active: boolean
  focused: boolean
  selected: boolean
  /** Some row is selected: every row shows its checkbox. */
  selecting: boolean
  /** The user has a draft in this thread: a red "Draft" (§9.1 U9). */
  draft?: boolean
  /** The Sent list: the recipients and the time of the last message sent. */
  sentView?: boolean
  /** The mailbox's name, in a list of several mailboxes (the all-mailbox Inbox, Starred, a label, search). */
  mailboxName?: string
  avatarOf: AvatarOf
  onOpen: (threadId: string) => void
  onSelect: (threadId: string) => void
  onStar: (thread: MailThreadSummary) => void
  /** The actions that show on hover, in place of the time. */
  onArchive: (thread: MailThreadSummary) => void
  onToggleRead: (thread: MailThreadSummary) => void
  onTrash: (thread: MailThreadSummary) => void
}

/** A small action of a row, shown on hover. */
function RowAction({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <Tip label={label}>
      <Button type="button" variant="ghost" size="icon-xs" className="text-muted-foreground hover:text-foreground" aria-label={label} onClick={onClick}>
        {children}
      </Button>
    </Tip>
  )
}

/**
 * A row of the thread list: sender, subject, snippet, time, label chips, star, unread dot, attachment icon, draft, the
 * mailbox when several show. On hover, Archive, Read and Trash take the time's place.
 */
export function ThreadRow({ thread, labels, active, focused, selected, selecting, draft = false, sentView = false, mailboxName, avatarOf, onOpen, onSelect, onStar, onArchive, onToggleRead, onTrash }: ThreadRowProps) {
  const chips = thread.labels.flatMap((id) => labels.get(id) ?? [])
  const time = (sentView && thread.last_sent_at) || thread.last_message_at
  const who = sentView ? sentRecipients(thread) : threadSenders(thread)
  return (
    <div
      data-slot="mail-thread-row"
      className="group/row relative flex min-h-16 w-full min-w-0 cursor-pointer items-start gap-2 border-b px-3 py-2.5 text-left transition-colors outline-none hover:bg-foreground/[0.02] focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset data-active:bg-muted data-focused:shadow-[inset_2px_0_0_var(--primary)] data-selected:bg-primary/5"
      data-active={active || undefined}
      data-focused={focused || undefined}
      data-selected={selected || undefined}
      data-selecting={selecting || undefined}
      data-unread={thread.unread || undefined}
      role="button"
      tabIndex={0}
      aria-label={`${thread.unread ? 'Unread, ' : ''}${who}: ${thread.subject || '(no subject)'}${draft ? ', draft' : ''}`}
      onClick={() => onOpen(thread.id)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && event.target === event.currentTarget) onOpen(thread.id)
      }}
    >
      <span className="flex max-h-8 w-2 shrink-0 items-center self-stretch">
        {thread.unread ? <span className="size-2 shrink-0 rounded-full bg-primary" /> : null}
      </span>
      <span className="relative flex size-8 shrink-0 items-center justify-center" onClick={(event) => event.stopPropagation()}>
        <span className="group-data-selecting/row:invisible hover-fine:group-hover/row:invisible">
          <UserAvatar
            user={sentView && thread.sent_to[0] ? avatarOf({ address: thread.sent_to[0] }, thread.sent_to[0]) : avatarOf(thread.from[0], personName(thread.from[0]))}
            size={32}
          />
        </span>
        <span className="invisible absolute inset-0 flex items-center justify-center group-data-selecting/row:visible hover-fine:group-hover/row:visible">
          <Checkbox aria-label="Select conversation" checked={selected} onCheckedChange={() => onSelect(thread.id)} />
        </span>
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-baseline gap-2">
          <span className="min-w-0 truncate text-[0.8125rem] font-normal text-foreground group-data-unread/row:font-semibold">
            {who}
            {thread.message_count > 1 ? <span className="ml-1 text-xs font-normal text-muted-foreground/70">{thread.message_count}</span> : null}
          </span>
          {draft ? <span className="shrink-0 text-[0.8125rem] font-normal text-destructive">Draft</span> : null}
          <span className="flex-1" />
          <span className="shrink-0 text-xs text-muted-foreground/70 hover-fine:group-hover/row:invisible" title={new Date(time).toLocaleString()}>
            {relativeTime(time)}
          </span>
        </div>
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="min-w-0 truncate text-[0.8125rem] text-foreground group-data-unread/row:font-semibold">{thread.subject || '(no subject)'}</span>
          {mailboxName ? <span className="shrink-0 rounded-sm bg-muted px-1.5 py-px text-[0.625rem] font-medium text-muted-foreground">{mailboxName}</span> : null}
        </div>
        <div className="truncate text-xs text-muted-foreground/70">{thread.snippet}</div>
        {chips.length > 0 ? (
          <div className="mt-1 flex min-w-0 flex-wrap gap-1">
            {chips.map((label) => (
              <span key={label.id} className="inline-flex h-5 max-w-32 items-center gap-1 rounded-full border px-1.5 text-[0.6875rem] text-muted-foreground">
                <ColorDot color={label.color} />
                <span className="truncate">{label.name}</span>
              </span>
            ))}
          </div>
        ) : null}
      </div>
      {/* over the time; only with a fine pointer (on touch the row opens) */}
      <div
        className="absolute top-1.5 right-10 hidden items-center rounded-md bg-background/95 shadow-xs ring-1 ring-border hover-fine:group-hover/row:flex hover-fine:group-focus-within/row:flex"
        onClick={(event) => event.stopPropagation()}
      >
        {thread.folder === 'archive' ? (
          <RowAction label="Move to inbox" onClick={() => onArchive(thread)}><DirectInbox className="size-3.5" /></RowAction>
        ) : thread.folder !== 'trash' && thread.folder !== 'spam' ? (
          <RowAction label="Archive" onClick={() => onArchive(thread)}><Archive className="size-3.5" /></RowAction>
        ) : null}
        <RowAction label={thread.unread ? 'Mark as read' : 'Mark as unread'} onClick={() => onToggleRead(thread)}>
          {thread.unread ? <Mail className="size-3.5" /> : <SmsTracking className="size-3.5" />}
        </RowAction>
        {thread.folder !== 'trash' ? <RowAction label="Move to trash" onClick={() => onTrash(thread)}><Trash className="size-3.5" /></RowAction> : null}
      </div>
      <div className="flex shrink-0 flex-col items-center gap-1 text-muted-foreground/70" onClick={(event) => event.stopPropagation()}>
        <Tip label={thread.starred ? 'Unstar' : 'Star'}>
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="text-muted-foreground/70"
            aria-label={thread.starred ? 'Unstar' : 'Star'}
            aria-pressed={thread.starred}
            onClick={() => onStar(thread)}
          >
            <Star className={cn('size-4', thread.starred && 'fill-[#fcd452] text-[#fcd452]')} />
          </Button>
        </Tip>
        {thread.has_attachments ? <Paperclip className="size-3.5" aria-label="Has attachments" /> : null}
      </div>
    </div>
  )
}
