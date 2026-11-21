// The mail composer (§9.1): one draft, saved on the server as the user types. `window` is the floating composer and
// `full` its expanded form (U2, both placed by the dock); new messages, replies and forwards all open there.
// ⌘Enter sends; the pre-send checks show above Send and ⌘Enter again sends anyway (U5). ⌘⇧Enter sends and archives
// the thread, ⌘⇧E expands (and back), Esc minimizes a window; the draft is kept, unless nothing was written in it.
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
  type ReactNode,
  type Ref,
} from 'react'
import { Danger, Paperclip2, TextBlock, Trash, Xmark as X } from 'reicon-react'
import { toast } from 'sonner'
import { cn } from 'cn'
import type { MailDraft, MailDraftFile, MailFromAddress, MailSnippet } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'
import { ShortcutKeys } from '@/shortcuts/Shortcut'
import { useIsPhone } from '@/features/chat/pages/useChatViewport'
import { problemText, type DraftText } from '../mailLib'
import { useDeleteDraft, useRemoveDraftFile, useSaveDraft, useSetDraftFileLink, useUploadDraftFile } from '../api/mutations'
import { fetchDraftFile, useDraft, useDraftCheck, useFromAddresses, useSnippets, useThread } from '../api/queries'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useQueryClient } from '@tanstack/react-query'
import { mailKeys } from '../api/keys'
import { isUntouchedDraft, notReadyText, recipientsKey, type SuppressedRecipient } from './composeLib'
import { createInlineImages } from './inlineImages'
import { ComposerFiles, type PendingUpload } from './ComposerFiles'
import { FromPicker } from './FromPicker'
import type { MailEditorHandle } from './MailEditor'
import { RecipientField, type RecipientFieldHandle } from './RecipientField'
import { SendButton } from './SendButton'
import { useComposerSend, type SentHandler } from './useComposerSend'

/** BlockNote is about 1 MB: the editor loads on first use (the app preloads it when idle). */
const LazyMailEditor = lazy(() => import('./MailEditor').then((module) => ({ default: module.MailEditor })))

export type ComposerPlacement = 'window' | 'full'

/** What a host can ask of an open composer. */
export interface ComposerHandle {
  /** Puts the focus back in the composer (the text, or To when there is nobody to send to). */
  focus: () => void
}

export interface ComposerProps {
  draftId: string
  placement: ComposerPlacement
  /**
   * Close the composer; the draft stays (what is not saved yet is saved on the way out). An untouched draft
   * (`isUntouchedDraft`: made just now, nothing changed) is deleted first: a reply opened by mistake leaves no "Draft"
   * behind.
   */
  onClose: () => void
  /** The server took the message (it waits for the undo time or its scheduled time). Without it a toast says so. */
  onSent?: SentHandler
  /** Esc (when no menu in the composer takes it). Without it, Esc closes the composer like `onClose`. */
  onEscape?: () => void
  /** ⌘⇧E: a window to full screen and back. */
  onPopOut?: () => void
  /** Focus the text (or To) when the composer opens. Off for windows brought back by a reload. */
  autoFocus?: boolean
  handleRef?: Ref<ComposerHandle>
  /** Window controls of the dock (minimize, expand), in the header of `window` and `full`. */
  headerActions?: ReactNode
  className?: string
}

const ROOT_CLASS: Record<ComposerPlacement, string> = {
  // on a phone the window fills the screen, as the full form does
  window: 'h-full min-h-0 overflow-hidden rounded-xl border bg-popover shadow-2xl max-[899px]:rounded-none max-[899px]:border-0 max-[899px]:bg-background max-[899px]:shadow-none',
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

function ComposerBody({
  draft,
  seed,
  fromAddresses,
  saver,
  onLoaded,
  isReloading,
  onRestore,
  draftId,
  placement,
  onClose,
  onSent,
  onEscape,
  onPopOut,
  autoFocus = true,
  handleRef,
  headerActions,
  className,
}: BodyProps) {
  const [initial] = useState(() => ({ ...draft, ...seed }))
  const [to, setTo] = useState<string[]>(initial.to)
  const [cc, setCc] = useState<string[]>(initial.cc)
  const [bcc, setBcc] = useState<string[]>(initial.bcc)
  const [subject, setSubject] = useState(initial.subject)
  const [fromId, setFromId] = useState(initial.from_address_id)
  const [showCc, setShowCc] = useState(initial.cc.length > 0)
  const [showBcc, setShowBcc] = useState(initial.bcc.length > 0)
  const noRecipients = initial.to.length + initial.cc.length + initial.bcc.length === 0
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

  /** Nothing changed since the composer opened, on a draft made just now: it can go when the composer closes. */
  const untouched = () =>
    saver.problem !== 'sending' &&
    !saver.restore &&
    isUntouchedDraft(initial, {
      to,
      cc,
      bcc,
      subject,
      fromId,
      bodyWritten: editor.current?.hasWriting(signatureOf(fromId)) ?? initial.body_html.trim() !== '',
      files: draft.files.length + pending.length,
    })

  /** The user wrote something: the signature put in at the start and a subject the server filled in (Re:, Fwd:) do not count. */
  const written = () => {
    const subjectWritten = draft.mode === 'new' ? subject.trim() !== '' : subject !== initial.subject
    return (editor.current?.hasWriting(signatureOf(fromId)) ?? initial.body_html.trim() !== '') || subjectWritten || draft.files.length > 0 || pending.length > 0
  }

  const discard = async () => {
    if (written() && !(await confirmAction({ title: 'Delete this draft?', description: 'Its text and files are deleted.', confirmLabel: 'Delete', danger: true }))) return
    saver.stop()
    deleteDraft.mutate(draftId, {
      onSuccess: () => {
        toast('Draft deleted')
        onClose()
      },
      onError: (error) => toast.error(problemText(error, 'The draft could not be deleted.')),
    })
  }

  // tidying up, not something the user asked for: a failure leaves an empty draft in Drafts and says nothing
  const deleteUntouched = () => deleteDraft.mutate(draftId, { onError: (error) => console.warn('An untouched draft could not be deleted.', error) })

  /** Closes the composer, deleting an untouched draft on the way. */
  const close = () => {
    if (untouched()) {
      saver.stop()
      deleteUntouched()
    }
    onClose()
  }

  useImperativeHandle(handleRef, () => ({
    focus: () => {
      if (to.length + cc.length + bcc.length === 0) toField.current?.focus()
      else editor.current?.focus()
    },
  }))

  // The composer's keys work while typing (they have modifiers, or Esc); they act on this composer only.
  const onKeyDownCapture = (event: KeyboardEvent<HTMLDivElement>) => {
    const mod = (event.metaKey || event.ctrlKey) && !event.altKey
    if (mod && event.key === 'Enter') {
      event.preventDefault()
      event.stopPropagation()
      // ⌘⇧Enter: send, then archive the thread and open the next one (a new message has no thread: it only sends)
      void send({ archive: event.shiftKey && Boolean(draft.thread_id) })
    } else if (mod && event.shiftKey && event.code === 'KeyE') {
      event.preventDefault()
      event.stopPropagation()
      onPopOut?.()
    }
  }
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // a menu inside (suggestions, the snippet list, a chip being edited) takes Esc first; menus in portals are not ours
    if (event.key !== 'Escape' || event.defaultPrevented || !(event.target instanceof Node) || !event.currentTarget.contains(event.target)) return
    if (editor.current?.menuOpen()) return
    event.preventDefault()
    event.stopPropagation()
    if (onEscape) onEscape()
    else close()
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

  // a phone shows the composer full screen, as the native app does: Close on the left, Send on the right
  const phone = useIsPhone()
  const title = subject.trim() || (draft.mode === 'new' ? 'New message' : draft.mode.startsWith('forward') ? 'Forward' : 'Reply')
  const showFrom = phone || fromAddresses.length > 1 || (fromAddress !== undefined && fromAddress.ready !== 'ready')
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

  return (
    <div data-slot="composer" data-placement={placement} className={cn('relative', className)} onKeyDownCapture={onKeyDownCapture} onKeyDown={onKeyDown} {...drag}>
      {phone ? (
        <header className="flex h-12 shrink-0 items-center gap-1 border-b border-border/60 px-2">
          <Button variant="ghost" size="icon" className="text-muted-foreground" aria-label="Close" onClick={close}>
            <X />
          </Button>
          <h2 className="min-w-0 flex-1 truncate text-[0.8125rem] font-semibold">{title}</h2>
          {headerActions}
          <SendButton blocked={blocked} sending={sending} anyway={warnings.length > 0} inThread={Boolean(draft.thread_id)} onSend={(options) => void send(options)} />
        </header>
      ) : (
        <header className="flex h-10 shrink-0 items-center gap-1 border-b border-border/60 pr-1.5 pl-3">
          <h2 className="min-w-0 flex-1 truncate text-[0.8125rem] font-semibold">{title}</h2>
          {headerActions}
          <Tip label="Close (the draft is kept)">
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground" aria-label="Close" onClick={close}>
              <X />
            </Button>
          </Tip>
        </header>
      )}

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

      {/* full screen: the fields, the text and the buttons in one centered column (the header spans the screen) */}
      <div className={cn('flex min-h-0 flex-1 flex-col', placement === 'full' && 'mx-auto w-full max-w-3xl')}>
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
            autoFocus={autoFocus && noRecipients}
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

        <div
          className="min-h-0 flex-1 cursor-text overflow-y-auto overscroll-contain px-3 py-3"
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
              autoFocus={autoFocus && !noRecipients}
              editable={!sending}
              className="min-h-full"
            />
          </Suspense>
        </div>

        {phone && draft.mode !== 'new' ? (
          <p className="shrink-0 px-3 pb-2 text-xs text-muted-foreground">
            {draft.mode.startsWith('forward') ? 'The forwarded message goes below your text.' : 'The message you answer is quoted below your text.'}
          </p>
        ) : null}

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
              {phone ? 'Tap Send anyway to send it.' : <>Press <ShortcutKeys keys="Mod+Enter" /> again to send anyway.</>}
            </p>
          </div>
        ) : null}

        <footer className="flex shrink-0 items-center gap-1 px-3 pb-3">
          {phone ? null : <SendButton blocked={blocked} sending={sending} anyway={warnings.length > 0} inThread={Boolean(draft.thread_id)} onSend={(options) => void send(options)} />}
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
      </div>

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
