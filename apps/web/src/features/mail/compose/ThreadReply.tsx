// The reply at the end of a thread (§9.1 U1): Reply / Reply all / Forward, or R / A / F, open it in place with the
// focus in the text. No motion: it opens many times a day, mostly from the keyboard. A thread has one draft for each
// member: a draft left earlier shows as a "Draft" line to continue; one popped out to a window says so.
import { useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type ReactNode, type Ref } from 'react'
import { ForwardRight, Reply } from 'reicon-react'
import { toast } from 'sonner'
import type { MailMessageView } from '@orbit/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import type { CommandId } from '@/shortcuts/commands'
import { Shortcut } from '@/shortcuts/Shortcut'
import { useCancelOutbound, useCreateDraft } from '../api/mutations'
import { useDrafts } from '../api/queries'
import { ApiProblem } from '@orbit/api/problem'
import { problemText, sendingOutboundId } from '../mailLib'
import { Composer, type ComposerHandle } from './Composer'
import { reopenDraft, useArchiveAfterSend, useComposerDock, useInlineReplyHost } from './useComposerDock'
import { draftText, recipientSummary } from './composeLib'
import { dismissUndoToast, useAnnounceSent } from './undoSend'
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
  const [sendingId, setSendingId] = useState<string | null>(null)
  const cancel = useCancelOutbound()
  const undoButton = useRef<HTMLButtonElement>(null)
  // Undo of a reply sent from here brings it back here
  useInlineReplyHost(threadId, (draftId) => {
    setSendingId(null)
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
    create.mutate(
      { mode, thread_id: threadId },
      {
        onSuccess: (draft) => {
          setSendingId(null)
          show(draft.id, click)
        },
        onError: (error) => {
          const held = sendingOutboundId(error)
          if (held) setSendingId(held)
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
        dismissUndoToast(outboundId)
        setSendingId(null)
        reopenDraft(draft, store, 'inline')
      },
      onError: (error) => {
        if (error instanceof ApiProblem && error.code === 'already_sent') {
          setSendingId(null)
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
    <div data-slot="thread-reply" data-print-hide="" className="flex flex-col gap-3 pt-4">
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
      <div ref={actions} className="flex flex-wrap items-center gap-2">
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
      <Button variant="outline" size="sm" disabled={disabled} onClick={onClick}>
        {icon}
        {label}
      </Button>
    </Tip>
  )
}
