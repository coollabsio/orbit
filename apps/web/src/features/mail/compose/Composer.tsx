// The mail composer (§9.1): one draft, saved on the server as the user types. `inline` is the reply at the end of a
// thread (recipients as one line until clicked, U1), `window` the floating composer and `full` its expanded form
// (U2, both placed by the dock). ⌘Enter sends; the pre-send checks show above Send and ⌘Enter again sends anyway (U5).
import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type DragEvent, type KeyboardEvent, type ReactNode } from 'react'
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
import { useDeleteDraft, useRemoveDraftFile, useSaveDraft, useSetDraftFileLink, useUploadDraftFile } from '../api/mutations'
import { fetchDraftFile, useDraft, useDraftCheck, useFromAddresses, useSnippets, useThread } from '../api/queries'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useQueryClient } from '@tanstack/react-query'
import { mailKeys } from '../api/keys'
import { notReadyText, recipientSummary, recipientsKey, type SuppressedRecipient } from './composeLib'
import { createInlineImages } from './inlineImages'
import { ComposerFiles, type PendingUpload } from './ComposerFiles'
import { FromPicker } from './FromPicker'
import type { MailEditorHandle } from './MailEditor'
import { RecipientField, type RecipientFieldHandle } from './RecipientField'
import { useComposerSend } from './useComposerSend'

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
  // set while the body is shown again: the old body must not save its text on the way out
  const reloading = useRef(false)
  const seenRestore = useRef(saver.restore)
  const isReloading = useCallback(() => reloading.current, [])
  const onLoaded = useCallback((version: number) => {
    loadedVersion.current = version
    reloading.current = false
  }, [])

  // A save came back 409 with a newer draft: the cache holds the server's copy, the user's text waits in `restore`.
  useEffect(() => {
    const restore = saver.restore
    if (restore === seenRestore.current) return
    seenRestore.current = restore
    if (!restore || saver.problem === 'sending' || !draft) return
    if (loadedVersion.current !== null && draft.version !== loadedVersion.current) {
      reloading.current = true
      setSeed(null)
      setGeneration((value) => value + 1)
    }
  }, [saver.restore, saver.problem, draft])

  const restoreText = () => {
    const text = saver.restore
    if (!text) return
    reloading.current = true
    setSeed(text)
    setGeneration((value) => value + 1)
    saver.applyRestore()
  }

  const root = cn('flex flex-col text-foreground', ROOT_CLASS[placement], className)
  if (draftQuery.isError && !draft) {
    // only a 404 means the draft is gone; anything else (offline, a server error) can be tried again
    const gone = draftQuery.error instanceof ApiProblem && draftQuery.error.status === 404
    return (
      <div className={root} data-slot="composer" data-placement={placement}>
        <div role="alert" className="flex items-center gap-3 p-4 text-[0.8125rem] text-muted-foreground">
          <span className="flex-1">{gone ? 'This draft is no longer here. It was sent or deleted.' : 'The draft could not be loaded.'}</span>
          {gone ? null : (
            <Button size="sm" variant="outline" disabled={draftQuery.isFetching} onClick={() => void draftQuery.refetch()}>
              Try again
            </Button>
          )}
          <Button size="sm" variant={gone ? 'outline' : 'ghost'} onClick={onClose}>
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
      onLoaded={onLoaded}
      isReloading={isReloading}
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
  /** This body is being replaced by a fresh one (conflict, Restore): it must not save its text. */
  isReloading: () => boolean
  onRestore: () => void
}

/** Exporting the body to HTML walks the whole document: it waits for a pause in typing. */
const BODY_EXPORT_MS = 400

const lower = (address: string) => address.trim().toLowerCase()
const isFileDrag = (event: DragEvent) => event.dataTransfer.types.includes('Files')

function ComposerBody({ draft, seed, fromAddresses, saver, onLoaded, isReloading, onRestore, draftId, placement, onClose, onSent, headerActions, className }: BodyProps) {
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
  const [pending, setPending] = useState<PendingUpload[]>([])
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set())
  const [dragging, setDragging] = useState(false)
  const dragDepth = useRef(0)
  const editor = useRef<MailEditorHandle | null>(null)
  const toField = useRef<RecipientFieldHandle>(null)
  const ccField = useRef<RecipientFieldHandle>(null)
  const bccField = useRef<RecipientFieldHandle>(null)
  const fileInput = useRef<HTMLInputElement>(null)
  const mounted = useRef(true)
  const workspaceId = useWorkspace().workspace.id
  const queryClient = useQueryClient()
  // previews of the images in the text; the object URLs go when the composer closes
  const [inlineImages] = useState(() => createInlineImages((fileId) => fetchDraftFile(workspaceId, draftId, fileId), draft.files))
  useEffect(() => inlineImages.setFiles(draft.files), [inlineImages, draft.files])
  useEffect(() => () => inlineImages.dispose(), [inlineImages])

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
  const snippets = useSnippets()
  const deleteDraft = useDeleteDraft()
  const upload = useUploadDraftFile(draftId)
  const removeFile = useRemoveDraftFile(draftId)
  const setLink = useSetDraftFileLink(draftId)
  // which saved recipients are suppressed: amber chips now, a warning before sending (U5)
  const check = useDraftCheck(draftId, recipientsKey(draft.to, draft.cc, draft.bcc))
  const suppressed = useMemo(
    () => new Map<string, SuppressedRecipient>((check.data?.suppressed ?? []).map((entry) => [lower(entry.address), entry])),
    [check.data],
  )

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
  const blocked =
    saver.problem === 'sending'
      ? 'This draft is being sent.'
      : fromAddress && fromAddress.ready !== 'ready'
        ? notReadyText(fromAddress.not_ready_reason)
        : pending.length > 0
          ? 'Wait for the files to finish uploading.'
          : null

  // The body's HTML, exported when typing pauses (or at once to send, or when the composer closes).
  const bodyExport = useRef<(() => string) | null>(null)
  const bodyTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const takeBody = () => {
    clearTimeout(bodyTimer.current)
    const exportHtml = bodyExport.current
    bodyExport.current = null
    return exportHtml
  }
  const saveBody = () => {
    const exportHtml = takeBody()
    if (exportHtml) saver.save({ body_html: exportHtml() })
  }
  const saveRef = useRef(saver.save)
  useEffect(() => {
    saveRef.current = saver.save
  })
  // On the way out the waiting export is always dropped (its timer would read a destroyed editor later). Closing
  // saves it first, in a layout cleanup: before the saver's last flush (a passive cleanup of the parent). A body
  // replaced after a conflict or Restore saves nothing: its text is old.
  useLayoutEffect(() => {
    const timer = bodyTimer
    const pendingExport = bodyExport
    return () => {
      clearTimeout(timer.current)
      const exportHtml = pendingExport.current
      pendingExport.current = null
      if (exportHtml && !isReloading()) saveRef.current({ body_html: exportHtml() })
    }
  }, [isReloading])
  const onBodyChange = (exportHtml: () => string) => {
    bodyExport.current = exportHtml
    clearTimeout(bodyTimer.current)
    bodyTimer.current = setTimeout(saveBody, BODY_EXPORT_MS)
  }

  const composer = useComposerSend({
    draft,
    saver,
    blocked,
    fromAddresses,
    // typed recipients become chips first (I1), so the send uses what the user sees
    collect: () => {
      takeBody()
      const lists = { to: toField.current?.commit() ?? to, cc: ccField.current?.commit() ?? cc, bcc: bccField.current?.commit() ?? bcc }
      return {
        ...lists,
        recipientsChanged: lists.to.length !== to.length || lists.cc.length !== cc.length || lists.bcc.length !== bcc.length,
        subject,
        fromId,
        html: editor.current?.html() ?? initial.body_html,
        text: editor.current?.text() ?? '',
      }
    },
    removeFiles: (ids) => Promise.all(ids.map((id) => removeFile.mutateAsync(id))),
    draftFiles: () => queryClient.getQueryData<MailDraft>(mailKeys.draft(workspaceId, draftId))?.files ?? draft.files,
    onRecipientProblem: () => {
      setFieldsOpen(true)
      requestAnimationFrame(() => toField.current?.focus())
    },
    onSent,
    onClose,
  })
  const { send, sending, warnings, notice, tooLarge, clearTooLarge: dismissTooLarge, recheck } = composer

  // Every change goes to the saver (2 s after the last one). A change to the fields or files checks again on the next
  // send; typing in the text does not (the warnings stay, and Send sends anyway).
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
    if (swapped) takeBody()
    saver.save(swapped ? { from_address_id: next, body_html: editor.current?.html() ?? '' } : { from_address_id: next })
  }

  // Files

  const addFiles = (list: readonly File[]) => {
    if (list.length === 0) return
    dismissTooLarge()
    recheck()
    for (const file of list) {
      const key = crypto.randomUUID()
      setPending((current) => [...current, { key, name: file.name, size: file.size }])
      upload
        .mutateAsync({ file })
        .catch((error: unknown) => toast.error(`${file.name}: ${problemText(error, 'The file could not be attached.')}`))
        .finally(() => {
          if (mounted.current) setPending((current) => current.filter((entry) => entry.key !== key))
        })
    }
  }
  /** Images put into the text go in it (after `blockId`); other files, and an image the server cannot show, are attached. */
  const addToText = (list: readonly File[], blockId: string | null) => {
    const images = list.filter((file) => file.type.startsWith('image/'))
    addFiles(list.filter((file) => !file.type.startsWith('image/')))
    if (images.length === 0) return
    recheck()
    for (const file of images) {
      const key = crypto.randomUUID()
      setPending((current) => [...current, { key, name: file.name, size: file.size }])
      upload
        .mutateAsync({ file, inline: true })
        .then((uploaded) => {
          if (!uploaded?.content_id || !mounted.current) return
          inlineImages.put(uploaded.content_id, file)
          editor.current?.insertImage(uploaded.content_id, file.name, blockId)
        })
        .catch((error: unknown) => {
          if (error instanceof ApiProblem && error.code === 'inline_not_image') {
            toast(`${file.name} is attached: only a PNG, JPEG, GIF or WebP image can go in the text.`)
            addFiles([file])
          } else {
            toast.error(`${file.name}: ${problemText(error, 'The image could not be added.')}`)
          }
        })
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
    dismissTooLarge()
    withBusy(file.id, setLink.mutateAsync({ fileId: file.id, asLink: !file.as_link }), 'The file could not be changed.')
  }
  const dropFile = (file: MailDraftFile) => {
    dismissTooLarge()
    withBusy(file.id, removeFile.mutateAsync(file.id), 'The file could not be removed.')
  }
  const linkSuggested = () => {
    const ids = tooLarge?.suggestion ?? []
    dismissTooLarge()
    for (const id of ids) withBusy(id, setLink.mutateAsync({ fileId: id, asLink: true }), 'The file could not be changed.')
  }

  const discard = async () => {
    // the signature put in at the start, a subject the server filled in (Re:, Fwd:) are not the user's writing
    const subjectWritten = draft.mode === 'new' ? subject.trim() !== '' : subject !== initial.subject
    const written = (editor.current?.hasWriting(signatureOf(fromId)) ?? initial.body_html.trim() !== '') || subjectWritten || files.length > 0
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
      const dropped = [...event.dataTransfer.files]
      // onto the text: images go in it where they were dropped; anywhere else every file is attached
      const onText = event.target instanceof Element && event.target.closest('[data-slot=mail-editor]')
      if (onText && editor.current) addToText(dropped, editor.current.blockAt({ x: event.clientX, y: event.clientY }))
      else addFiles(dropped)
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
            suppressed={suppressed}
            onName={learnName}
            ref={toField}
            autoFocus={noRecipients}
            trailing={!showCc || !showBcc ? ccToggles : null}
          />
          {showCc ? <RecipientField ref={ccField} label="Cc" addresses={cc} onChange={changeCc} names={names} suppressed={suppressed} onName={learnName} /> : null}
          {showBcc ? <RecipientField ref={bccField} label="Bcc" addresses={bcc} onChange={changeBcc} names={names} suppressed={suppressed} onName={learnName} /> : null}
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
            onPasteFiles={addToText}
            resolveImage={inlineImages.resolve}
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
              <span className="min-w-0">
                {warning.message}
                {warning.details ? (
                  <span className="mt-0.5 flex flex-col text-xs text-muted-foreground">
                    {warning.details.map((line) => (
                      <span key={line}>{line}</span>
                    ))}
                  </span>
                ) : null}
              </span>
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
            blocked ?? (
              <span className="flex items-center gap-2">
                Send <ShortcutKeys keys="Mod+Enter" />
              </span>
            )
          }
        >
          {/* aria-disabled keeps the reason reachable by hover and focus */}
          <Button
            data-slot="composer-send"
            aria-disabled={blocked ? true : undefined}
            className={cn('min-w-20 active:not-aria-[haspopup]:scale-[0.97]', blocked && 'cursor-not-allowed opacity-50 hover:translate-y-0 hover:bg-primary')}
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
        <SaveState saving={saver.saving} failed={saver.problem === 'error'} onRetry={() => {
            saveBody()
            void saver.flush()
          }} />
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
