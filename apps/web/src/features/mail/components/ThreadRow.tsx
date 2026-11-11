import { Paperclip2 as Paperclip, Star } from 'reicon-react'
import { cn } from 'cn'
import type { MailLabel, MailThreadSummary } from '@orbit/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { Tip } from '@/components/common/Tip'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { relativeTime } from '@/lib/format'
import { personName, threadSenders } from '@/features/mail/mailLib'

interface ThreadRowProps {
  thread: MailThreadSummary
  labels: ReadonlyMap<string, MailLabel>
  active: boolean
  focused: boolean
  selected: boolean
  /** Some row is selected: every row shows its checkbox. */
  selecting: boolean
  onOpen: (threadId: string) => void
  onSelect: (threadId: string) => void
  onStar: (thread: MailThreadSummary) => void
}

/** A row of the thread list: sender, subject, snippet, time, label chips, star, unread dot, attachment icon. */
export function ThreadRow({ thread, labels, active, focused, selected, selecting, onOpen, onSelect, onStar }: ThreadRowProps) {
  const chips = thread.labels.flatMap((id) => labels.get(id) ?? [])
  return (
    <div
      data-slot="mail-thread-row"
      className="group/row flex min-h-16 w-full min-w-0 cursor-pointer items-start gap-2 border-b px-3 py-2.5 text-left transition-colors outline-none hover:bg-foreground/[0.02] focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset data-active:bg-muted data-focused:shadow-[inset_2px_0_0_var(--primary)] data-selected:bg-primary/5"
      data-active={active || undefined}
      data-focused={focused || undefined}
      data-selected={selected || undefined}
      data-selecting={selecting || undefined}
      data-unread={thread.unread || undefined}
      role="button"
      tabIndex={0}
      aria-label={`${thread.unread ? 'Unread, ' : ''}${threadSenders(thread)}: ${thread.subject || '(no subject)'}`}
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
          <UserAvatar user={null} name={personName(thread.from[0])} size={32} />
        </span>
        <span className="invisible absolute inset-0 flex items-center justify-center group-data-selecting/row:visible hover-fine:group-hover/row:visible">
          <Checkbox aria-label="Select conversation" checked={selected} onCheckedChange={() => onSelect(thread.id)} />
        </span>
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-baseline gap-2">
          <span className="min-w-0 truncate text-[0.8125rem] font-normal text-foreground group-data-unread/row:font-semibold">
            {threadSenders(thread)}
            {thread.message_count > 1 ? <span className="ml-1 text-xs font-normal text-muted-foreground/70">{thread.message_count}</span> : null}
          </span>
          <span className="flex-1" />
          <span className="shrink-0 text-xs text-muted-foreground/70" title={new Date(thread.last_message_at).toLocaleString()}>
            {relativeTime(thread.last_message_at)}
          </span>
        </div>
        <div className="truncate text-[0.8125rem] text-foreground group-data-unread/row:font-semibold">{thread.subject || '(no subject)'}</div>
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
