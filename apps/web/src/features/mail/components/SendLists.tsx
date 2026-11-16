// The Drafts, Scheduled and Outbox folders (§9.1 U9, U11): not thread lists, but the user's drafts and the
// messages that wait to go. A draft opens in a window of the dock (or in its thread's reply when that thread is on
// screen); a message that still waits can be cancelled, and its draft opens again.
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { Trash } from 'reicon-react'
import { cn } from 'cn'
import type { MailDraft, MailOutboundItem } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { relativeTime } from '@/lib/format'
import { useCancelOutbound, useDeleteDraft } from '@/features/mail/api/mutations'
import { draftText, recipientSummary, sendTimeLabel } from '@/features/mail/compose/composeLib'
import { reopenDraft, useComposerDock } from '@/features/mail/compose/useComposerDock'
import { useMailTimeZone } from '@/features/mail/compose/useMailTimeZone'
import { problemText } from '@/features/mail/mailLib'
import { outboxStatus } from '@/features/mail/mailStatus'

const ROW =
  'flex w-full min-w-0 items-start gap-2 border-b px-3 py-2.5 text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset'

/** The user's drafts, last changed first: a click continues one, the bin deletes it. */
export function DraftList({ drafts }: { drafts: MailDraft[] }) {
  const { store } = useComposerDock()
  const remove = useDeleteDraft()
  const open = (draft: MailDraft) => reopenDraft(draft, store, 'inline', { animate: true })
  const sorted = [...drafts].sort((a, b) => b.updated_at.localeCompare(a.updated_at))
  return sorted.map((draft) => (
    <div
      key={draft.id}
      data-slot="mail-draft-row"
      role="button"
      tabIndex={0}
      className={cn(ROW, 'group/row cursor-pointer hover:bg-foreground/[0.02]')}
      aria-label={`Draft: ${draft.subject || '(no subject)'}`}
      onClick={() => open(draft)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && event.target === event.currentTarget) open(draft)
      }}
    >
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-baseline gap-2">
          <span className="shrink-0 text-[0.8125rem] font-semibold text-destructive">Draft</span>
          <span className="min-w-0 truncate text-[0.8125rem] text-foreground">{recipientSummary(draft.to, draft.cc, draft.bcc)}</span>
          <span className="flex-1" />
          <span className="shrink-0 text-xs text-muted-foreground/70" title={new Date(draft.updated_at).toLocaleString()}>
            {relativeTime(draft.updated_at)}
          </span>
        </div>
        <div className="truncate text-[0.8125rem] text-foreground">{draft.subject || '(no subject)'}</div>
        <div className="truncate text-xs text-muted-foreground/70">{draftText(draft.body_html) || ' '}</div>
      </div>
      <Tip label="Delete draft">
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="shrink-0 text-muted-foreground/70"
          aria-label="Delete draft"
          disabled={remove.isPending && remove.variables === draft.id}
          onClick={async (event) => {
            event.stopPropagation()
            if (!(await confirmAction({ title: 'Delete this draft?', description: 'Its text and files are deleted.', confirmLabel: 'Delete', danger: true }))) return
            remove.mutate(draft.id, {
              onSuccess: () => store.close(draft.id),
              onError: (error) => toast.error(problemText(error, 'The draft could not be deleted.')),
            })
          }}
        >
          <Trash className="size-4" />
        </Button>
      </Tip>
    </div>
  ))
}

/** The time, every second while a row counts down to its send. */
function useNow(ticking: boolean): Date {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    if (!ticking) return
    const timer = window.setInterval(() => setNow(new Date()), 1000)
    return () => window.clearInterval(timer)
  }, [ticking])
  return now
}

/**
 * Scheduled messages (when each goes) or the Outbox (messages in their undo time, over a limit, held by a problem,
 * or going): each says when it goes or why it waits. **Cancel send** brings a held one back as a draft, in a window.
 */
export function OutboundList({ view, items }: { view: 'scheduled' | 'outbox'; items: MailOutboundItem[] }) {
  const { store } = useComposerDock()
  const cancel = useCancelOutbound()
  const timeZone = useMailTimeZone()
  const now = useNow(items.some((item) => item.status === 'held' && !item.hold_reason && !item.scheduled))
  return items.map((item) => {
    const status = outboxStatus(item, now)
    const due = new Date(item.release_at)
    const text = view === 'scheduled' && !item.hold_reason && due > now ? `Sends ${sendTimeLabel(due, now, timeZone, true)}` : status.text
    return (
      <div key={item.id} data-slot="mail-outbound-row" className={ROW}>
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="min-w-0 truncate text-[0.8125rem] text-foreground">{recipientSummary(item.to, [], [])}</span>
          <span className="truncate text-[0.8125rem] text-foreground">{item.subject || '(no subject)'}</span>
          <span className={cn('text-xs', status.problem ? 'text-destructive' : 'text-muted-foreground/70')} title={due.toLocaleString()}>
            {text}
          </span>
        </div>
        {status.canCancel ? (
          <Tip label="It goes back to your drafts and opens">
            <Button
              type="button"
              variant="outline"
              size="xs"
              className="shrink-0"
              disabled={cancel.isPending && cancel.variables === item.id}
              onClick={() =>
                cancel.mutate(item.id, {
                  onSuccess: (draft) => store.open(draft.id, { animate: true }),
                  onError: (error) =>
                    toast.error(
                      error instanceof ApiProblem && error.code === 'already_sent'
                        ? 'Too late: the message was already sent.'
                        : problemText(error, 'The message could not be cancelled.'),
                    ),
                })
              }
            >
              Cancel send
            </Button>
          </Tip>
        ) : null}
      </div>
    )
  })
}
