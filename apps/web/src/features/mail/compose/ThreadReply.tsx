// The reply row at the end of a thread (§9.1 U1): Reply / Reply all / Forward, or R / A / F, open the reply in a
// composer window of the dock (full screen on a phone), as a new message does. A thread has one draft for each member:
// a draft left earlier shows as a "Draft" line to continue; one open in a window says so.
import { useEffect, useImperativeHandle, useRef, useState, type ReactNode, type Ref } from 'react'
import { ForwardRight, Reply } from 'reicon-react'
import { toast } from 'sonner'
import type { MailMessageView } from '@orbit/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import type { CommandId } from '@/shortcuts/commands'
import { Shortcut } from '@/shortcuts/Shortcut'
import { holdKeyboard } from '@/lib/holdKeyboard'
import { useCancelOutbound, useCreateDraft } from '../api/mutations'
import { mailKeys } from '../api/keys'
import { useDrafts, useOutbound } from '../api/queries'
import { heldReplyState } from '../mailStatus'
import { useQueryClient } from '@tanstack/react-query'
import { ApiProblem } from '@orbit/api/problem'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { problemText, sendingOutboundId } from '../mailLib'
import { useComposerDock } from './useComposerDock'
import { draftText, recipientSummary } from './composeLib'
import { endPendingSend } from './undoSend'

export type ReplyMode = 'reply' | 'reply_all' | 'forward'

export interface ThreadReplyHandle {
  /** R / A / F: opens the reply (or the thread's draft, wherever it is) and puts the focus in it. */
  reply: (mode: ReplyMode) => void
}

interface ThreadReplyProps {
  threadId: string
  /** The newest message: Reply all shows when it went to more than one person. */
  last: MailMessageView | undefined
  ref?: Ref<ThreadReplyHandle>
}

export function ThreadReply({ threadId, last, ref }: ThreadReplyProps) {
  const { state: dock, store } = useComposerDock()
  const create = useCreateDraft()
  // a draft left in this thread
  const drafts = useDrafts()
  // the reply of this thread is held for sending (undo time, scheduled, a hold): no draft opens until it is undone
  // (`since`: when the server said so; lists read before that say nothing about it)
  const [sending, setSending] = useState<{ id: string; since: number } | null>(null)
  const sendingId = sending?.id ?? null
  const queryClient = useQueryClient()
  const workspaceId = useWorkspace().workspace.id
  // the reply leaves the Outbox and Scheduled lists once it went or was undone (they follow `thread.updated`)
  const scheduledList = useOutbound('scheduled', undefined, sending !== null)
  const outboxList = useOutbound('outbox', undefined, sending !== null)
  const lists = [scheduledList, outboxList].map((list) => ({ items: list.data ?? [], updatedAt: list.data ? list.dataUpdatedAt : 0 }))
  if (sending && heldReplyState(sending.id, sending.since, lists) === 'gone') setSending(null)
  const cancel = useCancelOutbound()
  const undoButton = useRef<HTMLButtonElement>(null)

  const saved = drafts.data?.find((draft) => draft.thread_id === threadId)
  const savedInDock = saved !== undefined && dock.windows.some((entry) => entry.draftId === saved.id)

  /** R / A / F and Continue: one draft per thread, so the server answers with the existing one when there is one. */
  const start = (mode: string, click: boolean) => {
    if (create.isPending) return
    // a tap on a phone: the text field opens after the server answers, too late for iOS to show the keyboard
    if (click) holdKeyboard()
    create.mutate(
      { mode, thread_id: threadId },
      {
        onSuccess: (draft) => {
          setSending(null)
          store.open(draft.id, { animate: click })
        },
        onError: (error) => {
          const held = sendingOutboundId(error)
          if (held) {
            setSending({ id: held, since: Date.now() })
            void queryClient.invalidateQueries({ queryKey: mailKeys.outboundAll(workspaceId) })
          }
          else toast.error(problemText(error, 'The reply could not be started.'))
        },
      },
    )
  }

  // a key that ran into a held reply leads to its Undo
  useEffect(() => {
    if (sendingId) undoButton.current?.focus({ preventScroll: true })
  }, [sendingId])

  /** **Undo to edit**: the same as the send's Undo, the reply comes back here as a draft. */
  const undoToEdit = () => {
    if (!sendingId || cancel.isPending) return
    const outboundId = sendingId
    cancel.mutate(outboundId, {
      onSuccess: (draft) => {
        endPendingSend(outboundId)
        setSending(null)
        store.open(draft.id)
      },
      onError: (error) => {
        if (error instanceof ApiProblem && error.code === 'already_sent') {
          setSending(null)
          toast.error('Too late: the message was already sent.')
        } else toast.error(problemText(error, 'The message could not be stopped. Check Sent.'))
      },
    })
  }

  useImperativeHandle(ref, () => ({ reply: (mode) => start(mode, false) }))

  const replyAll = last ? last.to.length + last.cc.length > 1 : false

  return (
    // Reply, Reply all and Forward under the last message, after a line, as in the native app
    <div data-slot="thread-reply" data-print-hide="" className="mt-4 flex flex-col gap-3 border-t pt-4 pb-4">
      {saved && !savedInDock ? (
        <button
          type="button"
          className="flex min-w-0 items-baseline gap-2 rounded-xl border bg-card px-4 py-3 text-left text-[0.8125rem] shadow-xs transition-colors duration-100 outline-none hover:bg-muted/40 focus-visible:ring-3 focus-visible:ring-ring/50"
          onClick={() => start(saved.mode, true)}
          aria-label="Continue the draft"
        >
          <span className="shrink-0 font-semibold text-destructive">Draft</span>
          <span className="shrink-0 text-muted-foreground">{recipientSummary(saved.to, saved.cc, saved.bcc)}</span>
          <span className="min-w-0 truncate text-foreground/80">{draftText(saved.body_html)}</span>
        </button>
      ) : null}
      {saved && savedInDock ? (
        <p className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
          Your reply is open in a window.
          <Button size="xs" variant="outline" onClick={() => store.open(saved.id, { animate: true })}>
            Show
          </Button>
        </p>
      ) : null}
      {sendingId ? (
        <p role="status" data-slot="reply-sending" className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
          This reply is being sent.
          <Button ref={undoButton} size="xs" variant="outline" disabled={cancel.isPending} onClick={undoToEdit}>
            Undo to edit
          </Button>
        </p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <ReplyButton label="Reply" command="mail.reply" icon={<Reply />} disabled={create.isPending} onClick={() => start('reply', true)} />
        {replyAll ? <ReplyButton label="Reply all" command="mail.replyAll" icon={<Reply />} disabled={create.isPending} onClick={() => start('reply_all', true)} /> : null}
        <ReplyButton label="Forward" command="mail.forward" icon={<ForwardRight />} disabled={create.isPending} onClick={() => start('forward', true)} />
      </div>
    </div>
  )
}

function ReplyButton({ label, command, icon, disabled, onClick }: { label: string; command: CommandId; icon: ReactNode; disabled: boolean; onClick: () => void }) {
  return (
    <Tip
      label={
        <span className="flex items-center gap-2">
          {label} <Shortcut id={command} />
        </span>
      }
    >
      <Button variant="outline" disabled={disabled} onClick={onClick}>
        {icon}
        {label}
      </Button>
    </Tip>
  )
}
