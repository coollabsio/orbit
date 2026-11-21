// The Drafts, Scheduled and Outbox folders (§9.1 U9, U11): not thread lists, but the user's drafts and the
// messages that wait to go. A draft opens in a window of the dock (or in its thread's reply when that thread is on
// screen); a message that still waits can be cancelled, and its draft opens again. j / k move a focus through the
// rows like the thread list (the mail page owns it).
import { useEffect, useRef, useState } from 'react'
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
import { useComposerDock } from '@/features/mail/compose/useComposerDock'
import { useMailTimeZone } from '@/features/mail/compose/useMailTimeZone'
import { problemText } from '@/features/mail/mailLib'
import { outboxStatus } from '@/features/mail/mailStatus'

const ROW =
  'flex w-full min-w-0 items-start gap-2 border-b px-3 py-2.5 text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset data-focused:shadow-[inset_2px_0_0_var(--primary)]'

interface RowFocus {
  /** The row j / k focused. */
  focusedId: string | null
  onFocus: (id: string) => void
}

/** j / k: the focused row stays in view. */
function useFocusedRow(focused: boolean) {
  const row = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (focused) row.current?.scrollIntoView({ block: 'nearest' })
  }, [focused])
  return row
}

/** The user's drafts, in the order given: a click continues one, the bin deletes it. */
export function DraftList({ drafts, focusedId, onFocus }: { drafts: MailDraft[] } & RowFocus) {
  return drafts.map((draft) => <DraftRow key={draft.id} draft={draft} focused={draft.id === focusedId} onFocus={onFocus} />)
}

function DraftRow({ draft, focused, onFocus }: { draft: MailDraft; focused: boolean; onFocus: (id: string) => void }) {
  const { store } = useComposerDock()
  const remove = useDeleteDraft()
  const row = useFocusedRow(focused)
  const open = () => {
    onFocus(draft.id)
    store.open(draft.id, { animate: true })
  }
  return (
    <div
      ref={row}
      data-slot="mail-draft-row"
      data-focused={focused || undefined}
      role="button"
      tabIndex={0}
      className={cn(ROW, 'cursor-pointer hover:bg-foreground/[0.02]')}
      aria-label={`Draft: ${draft.subject || '(no subject)'}`}
      onClick={open}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && event.target === event.currentTarget) open()
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
          disabled={remove.isPending}
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
  )
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
export function OutboundList({ view, items, focusedId, onFocus }: { view: 'scheduled' | 'outbox'; items: MailOutboundItem[] } & RowFocus) {
  const now = useNow(items.some((item) => item.status === 'held' && !item.hold_reason && !item.scheduled))
  return items.map((item) => <OutboundRow key={item.id} view={view} item={item} now={now} focused={item.id === focusedId} onFocus={onFocus} />)
}

function OutboundRow({
  view,
  item,
  now,
  focused,
  onFocus,
}: {
  view: 'scheduled' | 'outbox'
  item: MailOutboundItem
  now: Date
  focused: boolean
  onFocus: (id: string) => void
}) {
  const { store } = useComposerDock()
  const cancel = useCancelOutbound()
  const timeZone = useMailTimeZone()
  const row = useFocusedRow(focused)
  const status = outboxStatus(item, now)
  const due = new Date(item.release_at)
  const text = view === 'scheduled' && !item.hold_reason && due > now ? `Sends ${sendTimeLabel(due, now, timeZone, true)}` : status.text
  return (
    <div ref={row} data-slot="mail-outbound-row" data-focused={focused || undefined} className={ROW} onClick={() => onFocus(item.id)}>
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
            disabled={cancel.isPending}
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
}
