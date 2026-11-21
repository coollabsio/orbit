// The reply at the end of a thread (§9.1 U1): Reply / Reply all / Forward, or R / A / F, open it in place with the
// focus in the text. No motion: it opens many times a day, mostly from the keyboard. A thread has one draft for each
// member: a draft left earlier shows as a "Draft" line to continue; one popped out to a window says so.
import { useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type ReactNode, type Ref } from 'react'
import { createPortal } from 'react-dom'
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
import { useIsPhone } from '@/features/chat/pages/useChatViewport'
import { personName, problemText, sendingOutboundId } from '../mailLib'
import { Composer, type ComposerHandle } from './Composer'
import { reopenDraft, useArchiveAfterSend, useComposerDock, useInlineReplyHost } from './useComposerDock'
import { draftText, recipientSummary } from './composeLib'
import { endPendingSend, useAnnounceSent } from './undoSend'
import type { SentHandler } from './useComposerSend'

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
  const phone = useIsPhone()
  const create = useCreateDraft()
  const announce = useAnnounceSent()
  const archiveAfterSend = useArchiveAfterSend()
  // where the focus was before the reply opened: it goes back there when the reply closes
  const returnFocus = useRef<HTMLElement | null>(null)
  const [openId, setOpenId] = useState<string | null>(null)
  // a draft left in this thread; not read while writing here (every autosave would read the list again)
  const drafts = useDrafts(undefined, openId === null)
  const composer = useRef<ComposerHandle>(null)
  const box = useRef<HTMLDivElement>(null)
  const actions = useRef<HTMLDivElement>(null)
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
  // Undo of a reply sent from here brings it back here
  useInlineReplyHost(threadId, (draftId) => {
    setSending(null)
    setOpenId(draftId)
  })

  const saved = drafts.data?.find((draft) => draft.thread_id === threadId)
  const savedInDock = saved !== undefined && dock.windows.some((entry) => entry.draftId === saved.id)

  /** Shows a draft of this thread: in its window when it is in the dock (animated only for a click), else here. */
  const show = (draftId: string, click: boolean) => {
    if (store.has(draftId)) {
      store.open(draftId, { animate: click })
      return
    }
    returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    setOpenId(draftId)
  }

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
          show(draft.id, click)
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
        reopenDraft(draft, store, 'inline')
      },
      onError: (error) => {
        if (error instanceof ApiProblem && error.code === 'already_sent') {
          setSending(null)
          toast.error('Too late: the message was already sent.')
        } else toast.error(problemText(error, 'The message could not be stopped. Check Sent.'))
      },
    })
  }

  /**
   * The reply closed (Esc, Delete, sent): the focus goes back where it was. The Reply button that opened it is gone
   * with the reply; then the focus goes to the Reply button shown again in its place.
   */
  const collapse = () => {
    setOpenId(null)
    const target = returnFocus.current
    returnFocus.current = null
    if (!target) return
    requestAnimationFrame(() => {
      const next = target.isConnected ? target : actions.current?.querySelector<HTMLElement>('button:not([disabled])')
      next?.focus({ preventScroll: true })
    })
  }

  useImperativeHandle(ref, () => ({
    // already writing here: the key only brings the focus back
    reply: (mode) => (openId ? composer.current?.focus() : start(mode, false)),
  }))

  // the reply opens at the end of a long thread: bring it into view at once (no smooth scroll)
  useLayoutEffect(() => {
    if (openId) box.current?.scrollIntoView({ block: 'nearest' })
  }, [openId])

  const onSent: SentHandler = (outbound, _draft, { archive }) => {
    // ⌘⇧Enter: archived (and the next thread opens when the page still shows this one); Undo puts it back
    const undoArchive = archive ? archiveAfterSend(threadId) : undefined
    announce(outbound, (restored) => {
      undoArchive?.()
      reopenDraft(restored, store, 'inline')
    })
  }

  const popOut = async (how: 'key' | 'click') => {
    const id = openId
    if (!id) return
    // the window starts from the server copy: save what was typed here first
    await composer.current?.handOff()
    setOpenId(null)
    returnFocus.current = null
    store.popOut(id, { animate: how === 'click' })
  }

  const replyAll = last ? last.to.length + last.cc.length > 1 : false
  // the composer windows sit at the bottom right: the bar stays above them
  const dockOpen = dock.windows.length > 0
  // the reply goes to the sender of the newest message, unless the user sent it (then to its recipients)
  const replyTo = last && !last.delivery && last.from ? personName(last.from) : null

  if (openId && phone) {
    // a phone writes the reply full screen, as the native app does (the thread stays below it)
    return createPortal(
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Reply"
        data-slot="thread-reply"
        // the visible height (above the keyboard) and clear of the notch, the home bar and the rounded corners; with the
        // keyboard open, the home bar is under it
        className="fixed inset-x-0 top-0 z-50 flex h-[var(--app-height,100svh)] flex-col bg-background pt-[env(safe-area-inset-top,0px)] pr-[env(safe-area-inset-right,0px)] pb-[env(safe-area-inset-bottom,0px)] pl-[env(safe-area-inset-left,0px)] in-data-keyboard:pb-0"
      >
        <Composer key={openId} draftId={openId} placement="full" className="min-h-0 flex-1" handleRef={composer} onClose={collapse} onSent={onSent} />
      </div>,
      document.body,
    )
  }

  if (openId) {
    return (
      <div ref={box} data-slot="thread-reply" className="scroll-mb-6 pt-4">
        <Composer
          key={openId}
          draftId={openId}
          placement="inline"
          handleRef={composer}
          onClose={collapse}
          onSent={onSent}
          onPopOut={(how) => void popOut(how)}
        />
      </div>
    )
  }

  return (
    // the bar stays at the bottom of the pane while the thread scrolls: the reply is always one click (or R) away.
    // A phone shows the native app's row instead: Reply, Reply all and Forward under the last message, after a line.
    <div
      data-slot="thread-reply"
      data-print-hide=""
      className={
        phone
          ? 'mt-4 flex flex-col gap-3 border-t pt-4 pb-4'
          : `sticky bottom-0 z-10 -mx-5 -mb-8 flex flex-col gap-2 bg-linear-to-t from-background from-75% to-transparent px-5 pt-6 ${dockOpen ? 'pb-16' : 'pb-5'}`
      }
    >
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
      {phone ? (
        <div ref={actions} className="flex flex-wrap gap-2">
          <Button variant="outline" disabled={create.isPending} onClick={() => start('reply', true)}>
            <Reply />
            Reply
          </Button>
          {replyAll ? (
            <Button variant="outline" disabled={create.isPending} onClick={() => start('reply_all', true)}>
              <Reply />
              Reply all
            </Button>
          ) : null}
          <Button variant="outline" disabled={create.isPending} onClick={() => start('forward', true)}>
            <ForwardRight />
            Forward
          </Button>
        </div>
      ) : (
      <div ref={actions} className="flex min-w-0 items-center gap-1 rounded-xl border bg-card p-1 shadow-sm">
        <button
          type="button"
          disabled={create.isPending}
          onClick={() => start('reply', true)}
          className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-lg px-2.5 text-left text-[0.8125rem] text-muted-foreground transition-colors outline-none hover:bg-muted/60 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 disabled:opacity-50"
        >
          <Reply className="size-4 shrink-0" />
          <span className="min-w-0 flex-1 truncate">{replyTo ? `Reply to ${replyTo}…` : 'Reply…'}</span>
          <Shortcut id="mail.reply" className="shrink-0 opacity-70" />
        </button>
        {replyAll ? <ReplyButton label="Reply all" command="mail.replyAll" icon={<Reply />} disabled={create.isPending} onClick={() => start('reply_all', true)} /> : null}
        <ReplyButton label="Forward" command="mail.forward" icon={<ForwardRight />} disabled={create.isPending} onClick={() => start('forward', true)} />
      </div>
      )}
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
      <Button variant="ghost" size="sm" className="shrink-0 text-muted-foreground hover:text-foreground" disabled={disabled} onClick={onClick}>
        {icon}
        <span className="max-[1199px]:sr-only">{label}</span>
      </Button>
    </Tip>
  )
}
