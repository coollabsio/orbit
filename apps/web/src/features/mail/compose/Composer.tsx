// The mail composer (§9.1): one draft, saved on the server as the user types. `inline` is the reply at the end of a
// thread (recipients as one line until clicked, U1), `window` the floating composer and `full` its expanded form
// (U2, both placed by the dock). ⌘Enter sends; the pre-send checks show above Send and ⌘Enter again sends anyway (U5).
import { lazy, Suspense, useEffect, useMemo, useRef, useState, type DragEvent, type KeyboardEvent, type ReactNode } from 'react'
import { ChevronDown, Danger, Paperclip2, Send, TextBlock, Trash, Xmark as X } from 'reicon-react'
import { toast } from 'sonner'
import { cn } from 'cn'
import type { MailDraft, MailDraftFile, MailFromAddress, MailOutbound, MailSnippet } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'
import { ShortcutKeys } from '@/shortcuts/Shortcut'
import { problemText, type DraftText } from '../mailLib'
import { useDeleteDraft, useRemoveDraftFile, useSaveDraft, useSendDraft, useSetDraftFileLink, useUploadDraftFile } from '../api/mutations'
import { useDraft, useFromAddresses, useMailMe, useSnippets, useThread } from '../api/queries'
import {
  encodedSize,
  isValidAddress,
  likelyLimit,
  linkRefusal,
  MESSAGE_OVERHEAD_BYTES,
  notReadyText,
  preSendWarnings,
  recipientSummary,
  suggestLinks,
  type Warning,
} from './composeLib'
import { ComposerFiles, type PendingUpload, type TooLarge } from './ComposerFiles'
import { FromPicker } from './FromPicker'
import type { MailEditorHandle } from './MailEditor'
import { RecipientField } from './RecipientField'

/** BlockNote is about 1 MB: the editor loads on first use (the app preloads it when idle). */
const LazyMailEditor = lazy(() => import('./MailEditor').then((module) => ({ default: module.MailEditor })))

export type ComposerPlacement = 'inline' | 'window' | 'full'

export interface ComposerProps {
  draftId: string
  placement: ComposerPlacement
  /** Close the composer; the draft stays (what is not saved yet is saved on the way out). */
  onClose: () => void
  /** The server took the message (it waits for the undo time or its scheduled time). Without it a toast says so. */
  onSent?: (outbound: MailOutbound, draft: MailDraft) => void
  /** Window controls of the dock (minimize, expand), in the header of `window` and `full`. */
  headerActions?: ReactNode
  className?: string
}

const ROOT_CLASS: Record<ComposerPlacement, string> = {
  inline: 'rounded-xl border bg-card shadow-xs',
  window: 'h-full min-h-0 overflow-hidden rounded-xl border bg-popover shadow-2xl',
  full: 'h-full min-h-0 bg-background',
}

export function Composer(props: ComposerProps) {
  const { draftId, placement, onClose, className } = props
  const draftQuery = useDraft(draftId)
  const draft = draftQuery.data
  const fromQuery = useFromAddresses(draft?.thread_id ?? undefined)
  const saver = useSaveDraft(draftId)
  // A conflict (or Restore) shows the draft again: the body below is keyed by it, so the editor starts over.
  const [generation, setGeneration] = useState(0)
  const [seed, setSeed] = useState<DraftText | null>(null)
  const loadedVersion = useRef<number | null>(null)
  const seenRestore = useRef(saver.restore)

  // A save came back 409 with a newer draft: the cache holds the server's copy, the user's text waits in `restore`.
  useEffect(() => {
    const restore = saver.restore
    if (restore === seenRestore.current) return
    seenRestore.current = restore
    if (!restore || saver.problem === 'sending' || !draft) return
    if (loadedVersion.current !== null && draft.version !== loadedVersion.current) {
      setSeed(null)
      setGeneration((value) => value + 1)
    }
  }, [saver.restore, saver.problem, draft])

  const restoreText = () => {
    const text = saver.restore
    if (!text) return
    setSeed(text)
    setGeneration((value) => value + 1)
    saver.applyRestore()
  }

  const root = cn('flex flex-col text-foreground', ROOT_CLASS[placement], className)
  if (draftQuery.isError) {
    return (
      <div className={root} data-slot="composer" data-placement={placement}>
        <div className="flex items-center gap-3 p-4 text-[0.8125rem] text-muted-foreground">
          <span className="flex-1">This draft is no longer here. It was sent or deleted.</span>
          <Button size="sm" variant="outline" onClick={onClose}>
            Close
          </Button>
        </div>
      </div>
    )
  }
  if (!draft || fromQuery.isPending) {
    return (
      <div className={root} data-slot="composer" data-placement={placement} aria-busy="true">
        <div className="flex flex-col gap-2.5 p-4">
          <Skeleton className="h-4 w-1/3" />
          <Skeleton className="h-4 w-2/3" />
          <Skeleton className="h-20 w-full" />
        </div>
      </div>
    )
  }
  return (
    <ComposerBody
      key={generation}
      {...props}
      className={root}
      draft={draft}
      seed={seed}
      fromAddresses={fromQuery.data ?? []}
      saver={saver}
      onLoaded={(version) => {
        loadedVersion.current = version
      }}
      onRestore={restoreText}
    />
  )
}

type Saver = ReturnType<typeof useSaveDraft>

interface BodyProps extends ComposerProps {
  /** The cached draft (live: its files change as uploads finish). Its text is read once, at mount. */
  draft: MailDraft
  /** Text to start from instead of the draft's (Restore my text). */
  seed: DraftText | null
  fromAddresses: readonly MailFromAddress[]
  saver: Saver
  onLoaded: (version: number) => void
  onRestore: () => void
}

const lower = (address: string) => address.trim().toLowerCase()
const utf8Length = (text: string) => new TextEncoder().encode(text).length
const isFileDrag = (event: DragEvent) => event.dataTransfer.types.includes('Files')

function ComposerBody({ draft, seed, fromAddresses, saver, onLoaded, onRestore, draftId, placement, onClose, onSent, headerActions, className }: BodyProps) {
  const [initial] = useState(() => ({ ...draft, ...seed }))
  const [to, setTo] = useState<string[]>(initial.to)
  const [cc, setCc] = useState<string[]>(initial.cc)
  const [bcc, setBcc] = useState<string[]>(initial.bcc)
  const [subject, setSubject] = useState(initial.subject)
  const [fromId, setFromId] = useState(initial.from_address_id)
  const [showCc, setShowCc] = useState(initial.cc.length > 0)
  const [showBcc, setShowBcc] = useState(initial.bcc.length > 0)
  const noRecipients = initial.to.length + initial.cc.length + initial.bcc.length === 0
  // inline: one recipients line until clicked (U1); open at once when there is nobody to send to (a forward)
  const [fieldsOpen, setFieldsOpen] = useState(placement !== 'inline' || noRecipients)
  const [learnedNames, setLearnedNames] = useState<ReadonlyMap<string, string>>(new Map())
  const [warnings, setWarnings] = useState<Warning[]>([])
  const [notice, setNotice] = useState<string | null>(null)
  const [tooLarge, setTooLarge] = useState<TooLarge | null>(null)
  const [pending, setPending] = useState<PendingUpload[]>([])
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set())
  const [sending, setSending] = useState(false)
  const [dragging, setDragging] = useState(false)
  const dragDepth = useRef(0)
  const editor = useRef<MailEditorHandle | null>(null)
  const toInput = useRef<HTMLInputElement>(null)
  const fileInput = useRef<HTMLInputElement>(null)
  const mounted = useRef(true)

  // the version this body started from: a later 409 with another version shows the server's copy
  const [startVersion] = useState(draft.version)
  useEffect(() => onLoaded(startVersion), [onLoaded, startVersion])
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])

  const thread = useThread(draft.thread_id ?? null)
  const allFrom = useFromAddresses()
  const me = useMailMe()
  const snippets = useSnippets()
  const sendDraft = useSendDraft()
  const deleteDraft = useDeleteDraft()
  const upload = useUploadDraftFile(draftId)
  const removeFile = useRemoveDraftFile(draftId)
  const setLink = useSetDraftFileLink(draftId)

  const messages = thread.data?.messages
  /** Names of the thread's people and of picked contacts, by lower-case address. */
  const names = useMemo(() => {
    const map = new Map<string, string>()
    for (const message of messages ?? []) {
      for (const person of [message.from, ...message.to, ...message.cc]) {
        if (person?.name?.trim()) map.set(lower(person.address), person.name.trim())
      }
    }
    for (const [address, name] of learnedNames) map.set(address, name)
    return map
  }, [messages, learnedNames])
  const learnName = (address: string, name: string) => setLearnedNames((current) => new Map(current).set(lower(address), name))

  const fromAddress = fromAddresses.find((address) => address.address_id === fromId)
  const signatureOf = (addressId: string) => fromAddresses.find((address) => address.address_id === addressId)?.signature_html?.trim() || null
  const files = draft.files
  const hardBlock =
    saver.problem === 'sending'
      ? 'This draft is being sent.'
      : fromAddress && fromAddress.ready !== 'ready'
        ? notReadyText(fromAddress.not_ready_reason)
        : pending.length > 0
          ? 'Wait for the files to finish uploading.'
          : null

  // Every change goes to the saver (2 s after the last one). A change to the fields or files checks again on the next
  // send; typing in the text does not (the warnings stay, and Send sends anyway).
  const recheck = () => {
    setWarnings([])
    setNotice(null)
  }
  const changeTo = (next: string[]) => {
    setTo(next)
    recheck()
    saver.save({ to: next })
  }
  const changeCc = (next: string[]) => {
    setCc(next)
    recheck()
    saver.save({ cc: next })
  }
  const changeBcc = (next: string[]) => {
    setBcc(next)
    recheck()
    saver.save({ bcc: next })
  }
  const changeSubject = (next: string) => {
    setSubject(next)
    recheck()
    saver.save({ subject: next })
  }
  const changeFrom = (next: string) => {
    if (next === fromId) return
    recheck()
    // the signature follows the address (U7), unless the user changed it
    const swapped = editor.current?.swapSignature(signatureOf(fromId), signatureOf(next)) ?? false
    setFromId(next)
    saver.save(swapped ? { from_address_id: next, body_html: editor.current?.html() ?? '' } : { from_address_id: next })
  }
  const onBodyChange = () => {
    const html = editor.current?.html()
    if (html !== undefined) saver.save({ body_html: html })
  }

  // Files

  const addFiles = (list: readonly File[]) => {
    if (list.length === 0) return
    setTooLarge(null)
    recheck()
    for (const file of list) {
      const key = crypto.randomUUID()
      setPending((current) => [...current, { key, name: file.name, size: file.size }])
      upload
        .mutateAsync(file)
        .catch((error: unknown) => toast.error(`${file.name}: ${problemText(error, 'The file could not be attached.')}`))
        .finally(() => {
          if (mounted.current) setPending((current) => current.filter((entry) => entry.key !== key))
        })
    }
  }
  const withBusy = (fileId: string, work: Promise<unknown>, failure: string) => {
    setBusy((current) => new Set(current).add(fileId))
    work
      .catch((error: unknown) => toast.error(problemText(error, failure)))
      .finally(() => {
        if (!mounted.current) return
        setBusy((current) => {
          const next = new Set(current)
          next.delete(fileId)
          return next
        })
      })
  }
  const toggleLink = (file: MailDraftFile) => {
    setTooLarge(null)
    withBusy(file.id, setLink.mutateAsync({ fileId: file.id, asLink: !file.as_link }), 'The file could not be changed.')
  }
  const dropFile = (file: MailDraftFile) => {
    setTooLarge(null)
    withBusy(file.id, removeFile.mutateAsync(file.id), 'The file could not be removed.')
  }
  const linkSuggested = () => {
    const ids = tooLarge?.suggestion ?? []
    setTooLarge(null)
    for (const id of ids) withBusy(id, setLink.mutateAsync({ fileId: id, asLink: true }), 'The file could not be changed.')
  }

  // Send

  const send = async (scheduledAt?: Date) => {
    if (sending) return
    if (hardBlock) {
      setNotice(hardBlock)
      return
    }
    const recipients = [...to, ...cc, ...bcc]
    if (recipients.length === 0 || recipients.some((address) => !isValidAddress(address))) {
      setNotice(recipients.length === 0 ? 'Add a recipient.' : 'Fix the address marked in red.')
      setFieldsOpen(true)
      requestAnimationFrame(() => toInput.current?.focus())
      return
    }
    const html = editor.current?.html() ?? initial.body_html
    // U5: the first ⌘Enter shows the checks, the next one sends anyway
    if (warnings.length === 0) {
      const found = preSendWarnings({
        text: editor.current?.text() ?? '',
        subject,
        fileCount: files.length,
        mode: draft.mode,
        to,
        cc,
        bcc,
        threadAddresses: messages ? messages.flatMap((message) => [message.from, ...message.to, ...message.cc]).flatMap((person) => (person ? [person.address] : [])) : null,
        workspaceDomains: workspaceDomains(allFrom.data ?? fromAddresses, me.data?.mailboxes.map((mailbox) => mailbox.primary_address ?? '') ?? [], messages),
        // members cannot read the suppression list; the server skips those recipients when sending
        suppressed: [],
      })
      if (found.length > 0) {
        setWarnings(found)
        setNotice(null)
        return
      }
    }
    setSending(true)
    setNotice(null)
    setTooLarge(null)
    try {
      const before = saver.current().restore
      saver.save({ to, cc, bcc, subject, from_address_id: fromId, body_html: html })
      await saver.flush()
      const after = saver.current()
      if (after.problem || after.restore !== before) {
        setNotice(
          after.problem === 'sending'
            ? 'This draft is being sent.'
            : after.problem === 'error'
              ? 'The draft could not be saved. Try again.'
              : 'This draft changed on another device. Check it, then send again.',
        )
        return
      }
      const outbound = await sendDraft.mutateAsync({ draftId, body: scheduledAt ? { scheduled_at: scheduledAt.toISOString() } : undefined })
      saver.stop()
      const sent: MailDraft = { ...draft, to, cc, bcc, subject, from_address_id: fromId, body_html: html }
      if (onSent) onSent(outbound, sent)
      else toast.success(outbound.scheduled ? 'Message scheduled' : 'Message sent')
      onClose()
    } catch (error) {
      if (error instanceof ApiProblem && error.code === 'message_too_large') {
        const sized = files.map((file) => ({ id: file.id, size: file.size, asLink: file.as_link, linkable: linkRefusal(file) === null }))
        const bodyBytes = MESSAGE_OVERHEAD_BYTES + utf8Length(subject) + utf8Length(html)
        const size = encodedSize(sized, bodyBytes)
        const limit = likelyLimit(size)
        setTooLarge({ size, limit, suggestion: suggestLinks(sized, limit, bodyBytes) })
      } else {
        setNotice(problemText(error, 'The message could not be sent. Try again.'))
      }
    } finally {
      if (mounted.current) setSending(false)
    }
  }

  const discard = async () => {
    const written = (editor.current?.text().trim() ?? '') !== '' || subject.trim() !== '' || files.length > 0
    if (written && !(await confirmAction({ title: 'Delete this draft?', description: 'Its text and files are deleted.', confirmLabel: 'Delete', danger: true }))) return
    saver.stop()
    deleteDraft.mutate(draftId, {
      onSuccess: () => {
        toast('Draft deleted')
        onClose()
      },
      onError: (error) => toast.error(problemText(error, 'The draft could not be deleted.')),
    })
  }

  const onKeyDownCapture = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Enter' && (event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey) {
      event.preventDefault()
      event.stopPropagation()
      void send()
    }
  }

  // Files dropped anywhere on the composer are attached (U8).
  const drag = {
    onDragEnterCapture: (event: DragEvent<HTMLDivElement>) => {
      if (!isFileDrag(event)) return
      event.preventDefault()
      dragDepth.current += 1
      setDragging(true)
    },
    onDragLeaveCapture: (event: DragEvent<HTMLDivElement>) => {
      if (!isFileDrag(event)) return
      dragDepth.current = Math.max(0, dragDepth.current - 1)
      if (dragDepth.current === 0) setDragging(false)
    },
    onDragOverCapture: (event: DragEvent<HTMLDivElement>) => {
      if (!isFileDrag(event)) return
      event.preventDefault()
      event.dataTransfer.dropEffect = 'copy'
    },
    onDropCapture: (event: DragEvent<HTMLDivElement>) => {
      if (!isFileDrag(event)) return
      event.preventDefault()
      event.stopPropagation()
      dragDepth.current = 0
      setDragging(false)
      addFiles([...event.dataTransfer.files])
    },
  }

  const inline = placement === 'inline'
  const title = subject.trim() || (draft.mode === 'new' ? 'New message' : draft.mode.startsWith('forward') ? 'Forward' : 'Reply')
  const showFrom = fromAddresses.length > 1 || (fromAddress !== undefined && fromAddress.ready !== 'ready')
  const ccToggles = (
    <span className="mt-[5px] flex shrink-0 items-center gap-0.5">
      {!showCc ? (
        <button type="button" className="rounded-md px-1.5 py-0.5 text-xs text-muted-foreground transition-colors duration-100 hover:bg-muted hover:text-foreground" onClick={() => setShowCc(true)}>
          Cc
        </button>
      ) : null}
      {!showBcc ? (
        <button type="button" className="rounded-md px-1.5 py-0.5 text-xs text-muted-foreground transition-colors duration-100 hover:bg-muted hover:text-foreground" onClick={() => setShowBcc(true)}>
          Bcc
        </button>
      ) : null}
    </span>
  )
  const named = (address: string) => ({ address, name: names.get(lower(address)) })

  return (
    <div data-slot="composer" data-placement={placement} className={cn('relative', className)} onKeyDownCapture={onKeyDownCapture} {...drag}>
      {!inline ? (
        <header className="flex h-10 shrink-0 items-center gap-1 border-b border-border/60 pr-1.5 pl-3">
          <h2 className="min-w-0 flex-1 truncate text-[0.8125rem] font-semibold">{title}</h2>
          {headerActions}
          <Tip label="Close">
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground" aria-label="Close" onClick={onClose}>
              <X />
            </Button>
          </Tip>
        </header>
      ) : null}

      {saver.problem === 'sending' ? (
        <Banner>This draft is being sent from another window. Your changes are kept.</Banner>
      ) : saver.restore ? (
        <Banner
          actions={
            <>
              <Button size="xs" variant="outline" onClick={onRestore}>
                Restore my text
              </Button>
              <Button size="xs" variant="ghost" onClick={saver.dismissRestore}>
                Dismiss
              </Button>
            </>
          }
        >
          This draft changed on another device.
        </Banner>
      ) : null}

      {fieldsOpen ? (
        <div className="shrink-0">
          {showFrom ? <FromPicker addresses={fromAddresses} value={fromId} onChange={changeFrom} /> : null}
          <RecipientField
            label="To"
            addresses={to}
            onChange={changeTo}
            names={names}
            onName={learnName}
            inputRef={toInput}
            autoFocus={noRecipients}
            trailing={!showCc || !showBcc ? ccToggles : null}
          />
          {showCc ? <RecipientField label="Cc" addresses={cc} onChange={changeCc} names={names} onName={learnName} /> : null}
          {showBcc ? <RecipientField label="Bcc" addresses={bcc} onChange={changeBcc} names={names} onName={learnName} /> : null}
          <div className="flex min-h-9 items-center border-b border-border/60 px-3">
            <input
              value={subject}
              onChange={(event) => changeSubject(event.target.value)}
              placeholder="Subject"
              aria-label="Subject"
              className="h-9 w-full bg-transparent text-[0.8125rem] font-medium text-foreground outline-none placeholder:font-normal placeholder:text-muted-foreground/60"
            />
          </div>
        </div>
      ) : (
        <button
          type="button"
          className="flex h-9 shrink-0 items-center gap-1.5 border-b border-border/60 px-3 text-left text-[0.8125rem] text-muted-foreground transition-colors duration-100 outline-none hover:text-foreground focus-visible:bg-muted/50"
          onClick={() => setFieldsOpen(true)}
          aria-label="Edit recipients"
        >
          <span className="min-w-0 truncate">{recipientSummary(to.map(named), cc.map(named), bcc.map(named))}</span>
          <ChevronDown className="size-3.5 shrink-0" />
        </button>
      )}

      <div
        className={cn('min-h-0 cursor-text px-3 py-3', inline ? 'min-h-32' : 'flex-1 overflow-y-auto overscroll-contain', placement === 'full' && 'mx-auto w-full max-w-3xl')}
        onMouseDown={(event) => {
          // a press in the padding or below the text goes to the text
          if (event.target !== event.currentTarget) return
          event.preventDefault()
          editor.current?.focus()
        }}
      >
        <Suspense fallback={<EditorFallback />}>
          <LazyMailEditor
            ref={editor}
            initialHtml={initial.body_html}
            signatureHtml={signatureOf(initial.from_address_id)}
            snippets={snippets.data ?? NO_SNIPPETS}
            onChange={onBodyChange}
            onFiles={addFiles}
            autoFocus={!noRecipients}
            editable={!sending}
            className={inline ? 'min-h-24' : 'min-h-full'}
          />
        </Suspense>
      </div>

      <ComposerFiles files={files} pending={pending} busy={busy} tooLarge={tooLarge} onRemove={dropFile} onToggleLink={toggleLink} onLinkSuggested={linkSuggested} />

      {warnings.length > 0 ? (
        <div role="status" className="mx-3 mb-2 flex flex-col gap-1 rounded-lg border border-amber-500/30 bg-amber-500/8 px-2.5 py-2 text-[0.8125rem]">
          {warnings.map((warning) => (
            <p key={warning.kind} className="flex items-start gap-2">
              <Danger className="mt-0.5 size-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
              <span className="min-w-0">{warning.message}</span>
            </p>
          ))}
          <p className="flex items-center gap-1.5 pl-5.5 text-xs text-muted-foreground">
            Press <ShortcutKeys keys="Mod+Enter" /> again to send anyway.
          </p>
        </div>
      ) : null}

      <footer className="flex shrink-0 items-center gap-1 px-3 pb-3">
        <Tip
          label={
            hardBlock ?? (
              <span className="flex items-center gap-2">
                Send <ShortcutKeys keys="Mod+Enter" />
              </span>
            )
          }
        >
          {/* aria-disabled keeps the reason reachable by hover and focus */}
          <Button
            data-slot="composer-send"
            aria-disabled={hardBlock ? true : undefined}
            className={cn('min-w-20 active:not-aria-[haspopup]:scale-[0.97]', hardBlock && 'cursor-not-allowed opacity-50 hover:translate-y-0 hover:bg-primary')}
            onClick={() => void send()}
          >
            {sending ? <Spinner className="size-3.5" /> : <Send />}
            {warnings.length > 0 ? 'Send anyway' : 'Send'}
          </Button>
        </Tip>
        <Tip label="Attach files">
          <Button variant="ghost" size="icon-sm" className="text-muted-foreground" aria-label="Attach files" onClick={() => fileInput.current?.click()}>
            <Paperclip2 />
          </Button>
        </Tip>
        {(snippets.data?.length ?? 0) > 0 ? (
          <Tip
            label={
              <span className="flex items-center gap-2">
                Snippets <ShortcutKeys keys=";" />
              </span>
            }
          >
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground" aria-label="Insert a snippet" onClick={() => editor.current?.openSnippets()}>
              <TextBlock />
            </Button>
          </Tip>
        ) : null}
        <input
          ref={fileInput}
          type="file"
          multiple
          hidden
          onChange={(event) => {
            addFiles([...(event.target.files ?? [])])
            event.target.value = ''
          }}
        />
        <p role="alert" className="min-w-0 flex-1 truncate px-1.5 text-xs text-destructive">
          {notice}
        </p>
        <SaveState saving={saver.saving} failed={saver.problem === 'error'} onRetry={() => void saver.flush()} />
        <Tip label="Delete draft">
          <Button variant="ghost" size="icon-sm" className="text-muted-foreground hover:text-destructive" aria-label="Delete draft" onClick={() => void discard()}>
            <Trash />
          </Button>
        </Tip>
      </footer>

      {dragging ? (
        <div className="pointer-events-none absolute inset-1 z-10 flex items-center justify-center rounded-[10px] border-2 border-dashed border-primary/50 bg-background/85 text-sm font-medium text-primary animate-in fade-in-0 duration-150 motion-reduce:animate-none">
          Drop files to attach
        </div>
      ) : null}
    </div>
  )
}

const NO_SNIPPETS: MailSnippet[] = []

/** The workspace's mail domains as the composer knows them: its own addresses, the mailboxes, inside senders. */
function workspaceDomains(
  addresses: readonly MailFromAddress[],
  mailboxAddresses: readonly string[],
  messages: readonly { from?: { address: string } | null; from_external: boolean }[] | undefined,
): string[] {
  const domains = new Set<string>()
  const add = (address: string) => {
    const domain = lower(address).split('@')[1]
    if (domain) domains.add(domain)
  }
  for (const address of addresses) add(address.address)
  for (const address of mailboxAddresses) add(address)
  for (const message of messages ?? []) if (message.from && !message.from_external) add(message.from.address)
  return [...domains]
}

function Banner({ children, actions }: { children: ReactNode; actions?: ReactNode }) {
  return (
    <div role="status" className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1.5 border-b border-amber-500/20 bg-amber-500/8 px-3 py-2 text-[0.8125rem]">
      <span className="min-w-0 flex-1">{children}</span>
      {actions ? <span className="flex shrink-0 items-center gap-1">{actions}</span> : null}
    </div>
  )
}

/** "Saving…" while a save runs, then "Saved"; a failed save offers a retry. */
function SaveState({ saving, failed, onRetry }: { saving: boolean; failed: boolean; onRetry: () => void }) {
  const [saved, setSaved] = useState(false)
  const wasSaving = useRef(saving)
  useEffect(() => {
    if (wasSaving.current && !saving && !failed) setSaved(true)
    wasSaving.current = saving
  }, [saving, failed])
  if (failed) {
    return (
      <button type="button" className="shrink-0 rounded-md px-1.5 py-0.5 text-xs text-destructive hover:bg-destructive/10" onClick={onRetry}>
        Not saved. Retry
      </button>
    )
  }
  return (
    <span aria-live="polite" className={cn('shrink-0 px-1 text-xs text-muted-foreground/70 transition-opacity duration-200', !saving && !saved && 'opacity-0')}>
      {saving ? 'Saving…' : 'Saved'}
    </span>
  )
}

function EditorFallback() {
  return (
    <div className="flex min-h-24 items-start py-1" aria-busy="true">
      <Spinner className="size-4 text-muted-foreground" />
    </div>
  )
}
