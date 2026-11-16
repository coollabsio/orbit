// The reply at the end of a thread (§9.1 U1): Reply / Reply all / Forward, or R / A / F, open it in place with the
// focus in the text. No motion: it opens many times a day, mostly from the keyboard. A thread has one draft for each
// member: a draft left earlier shows as a "Draft" line to continue; one popped out to a window says so.
import { useImperativeHandle, useLayoutEffect, useRef, useState, type ReactNode, type Ref } from 'react'
import { ForwardRight, Reply } from 'reicon-react'
import { toast } from 'sonner'
import type { MailMessageView } from '@orbit/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import type { CommandId } from '@/shortcuts/commands'
import { Shortcut } from '@/shortcuts/Shortcut'
import { useCreateDraft } from '../api/mutations'
import { useDrafts } from '../api/queries'
import { problemText } from '../mailLib'
import { Composer, type ComposerHandle } from './Composer'
import { reopenDraft, useComposerDock, useInlineReplyHost } from './useComposerDock'
import { recipientSummary } from './composeLib'
import { useAnnounceSent } from './undoSend'
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
  /** ⌘⇧Enter sent the reply: archive the thread and open the next one. */
  onSentArchive: () => void
  ref?: Ref<ThreadReplyHandle>
}

/** The text of a draft's body, for the "Draft" line. */
function plainText(html: string): string {
  if (!html) return ''
  const text = new DOMParser().parseFromString(html, 'text/html').body.textContent ?? ''
  return text.replace(/\s+/g, ' ').trim()
}

export function ThreadReply({ threadId, last, onSentArchive, ref }: ThreadReplyProps) {
  const { state: dock, store } = useComposerDock()
  const create = useCreateDraft()
  const announce = useAnnounceSent()
  const [openId, setOpenId] = useState<string | null>(null)
  // a draft left in this thread; not read while writing here (every autosave would read the list again)
  const drafts = useDrafts(undefined, openId === null)
  const composer = useRef<ComposerHandle>(null)
  const box = useRef<HTMLDivElement>(null)
  // Undo of a reply sent from here brings it back here
  useInlineReplyHost(threadId, setOpenId)

  const saved = drafts.data?.find((draft) => draft.thread_id === threadId)
  const savedInDock = saved !== undefined && dock.windows.some((entry) => entry.draftId === saved.id)

  /** Shows a draft of this thread: in its window when it is in the dock, else here. */
  const show = (draftId: string) => {
    if (store.has(draftId)) store.open(draftId)
    else setOpenId(draftId)
  }

  const start = (mode: ReplyMode) => {
    if (create.isPending) return
    // one draft per thread: the server answers with the existing one when there is one
    create.mutate({ mode, thread_id: threadId }, { onSuccess: (draft) => show(draft.id), onError: (error) => toast.error(problemText(error, 'The reply could not be started.')) })
  }

  useImperativeHandle(ref, () => ({
    // already writing here: the key only brings the focus back
    reply: (mode) => (openId ? composer.current?.focus() : start(mode)),
  }))

  // the reply opens at the end of a long thread: bring it into view at once (no smooth scroll)
  useLayoutEffect(() => {
    if (openId) box.current?.scrollIntoView({ block: 'nearest' })
  }, [openId])

  const onSent: SentHandler = (outbound, _draft, { archive }) => {
    announce(outbound, (restored) => reopenDraft(restored, store))
    if (archive) onSentArchive()
  }

  const popOut = async () => {
    const id = openId
    if (!id) return
    // the window starts from the server copy: save what was typed here first
    await composer.current?.handOff()
    setOpenId(null)
    store.popOut(id)
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
          onClose={() => setOpenId(null)}
          onSent={onSent}
          onPopOut={() => void popOut()}
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
          onClick={() => setOpenId(saved.id)}
          aria-label="Continue the draft"
        >
          <span className="shrink-0 font-semibold text-destructive">Draft</span>
          <span className="shrink-0 text-muted-foreground">{recipientSummary(saved.to, saved.cc, saved.bcc)}</span>
          <span className="min-w-0 truncate text-foreground/80">{plainText(saved.body_html)}</span>
        </button>
      ) : null}
      {saved && savedInDock ? (
        <p className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
          Your reply is open in a window.
          <Button size="xs" variant="outline" onClick={() => store.open(saved.id)}>
            Show
          </Button>
        </p>
      ) : null}
      <div className="flex flex-wrap items-center gap-2">
        <ReplyButton label="Reply" command="mail.reply" icon={<Reply />} disabled={create.isPending} onClick={() => start('reply')} />
        {replyAll ? <ReplyButton label="Reply all" command="mail.replyAll" icon={<Reply />} disabled={create.isPending} onClick={() => start('reply_all')} /> : null}
        <ReplyButton label="Forward" command="mail.forward" icon={<ForwardRight />} disabled={create.isPending} onClick={() => start('forward')} />
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
