// The floating composers at the bottom right (§9.1 U2). The dock lives in the app shell, so a window stays open in
// Tasks, Chat and Docs. At most three windows (those that fit the screen); the others are pills; one can be full screen.
// A minimized window keeps its editor (hidden), so minimize, expand and full screen never lose the caret or the undo
// history. Windows brought back by a reload mount when the server has confirmed their drafts still exist.
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import { useLocation } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { Edit, Maximize4, Minimize, Minus, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { ShortcutKeys } from '@/shortcuts/Shortcut'
import { useCommand } from '@/shortcuts/useCommand'
import { draftsQuery, useDraft, useMailMe } from '../api/queries'
import { Composer, type ComposerHandle } from './Composer'
import { dockCapacity, layoutDock, type ComposerDockStore, type DockWindow } from './composerDockStore'
import { useAnnounceSent, useUndoSendShortcut } from './undoSend'
import { reopenDraft, useArchiveAfterSend, useComposerDock, useNewEmail } from './useComposerDock'
import type { SentHandler } from './useComposerSend'

const subscribeResize = (onChange: () => void) => {
  window.addEventListener('resize', onChange)
  return () => window.removeEventListener('resize', onChange)
}
const viewportWidth = () => window.innerWidth

// the restored windows of each workspace are checked against the server once per page load
const checkedWorkspaces = new Set<string>()

/** The shell's dock. Mounted only while Mail is on (and not for guests). */
export function ComposerDock() {
  const workspaceId = useWorkspace().workspace.id
  const { state, store } = useComposerDock()
  const queryClient = useQueryClient()
  const { pathname } = useLocation()
  const me = useMailMe()
  const newEmail = useNewEmail()
  const canCompose = (me.data?.mailboxes.length ?? 0) > 0
  // the mail page has C for this (and its own ⌘K entry): elsewhere ⌘K offers "New email"
  useCommand('mail.composeAnywhere', () => void newEmail(), { enabled: canCompose && !pathname.startsWith('/mail') })
  useUndoSendShortcut()

  // Drafts sent or deleted elsewhere since the last visit leave the dock before their windows show.
  const [checked, setChecked] = useState(() => checkedWorkspaces.has(workspaceId) || store.restoredIds().length === 0)
  useEffect(() => {
    if (checked) return
    let current = true
    queryClient.fetchQuery({ ...draftsQuery(workspaceId), staleTime: 0 }).then(
      (drafts) => {
        if (current) store.dropMissing(drafts.map((draft) => draft.id))
      },
      // not readable now: the windows show, and a gone draft says so in its window
      () => {},
    ).finally(() => {
      if (!current) return
      checkedWorkspaces.add(workspaceId)
      setChecked(true)
    })
    return () => {
      current = false
    }
  }, [checked, queryClient, store, workspaceId])

  const width = useSyncExternalStore(subscribeResize, viewportWidth)
  const layout = layoutDock(state, dockCapacity(width))
  const visible = new Set([...layout.windows, ...(layout.full ? [layout.full] : [])])

  // A window keeps its composer once it has been on screen (minimized it is hidden, not unmounted).
  const [kept, setKept] = useState<ReadonlySet<string>>(() => new Set())
  const present = new Set(state.windows.map((entry) => entry.draftId))
  const nextKept = new Set([...kept].filter((id) => present.has(id)))
  if (checked) for (const id of visible) nextKept.add(id)
  if (nextKept.size !== kept.size || [...nextKept].some((id) => !kept.has(id))) setKept(nextKept)

  // windows opened before this dock mounted (a reload) do not take the focus from the page; any opened since do
  const [mountSequence] = useState(() => Math.max(0, ...store.getSnapshot().windows.map((entry) => entry.openedAt)))
  if (state.windows.length === 0) return null
  return (
    <div data-slot="composer-dock" className="pointer-events-none fixed right-4 bottom-4 z-40 flex items-end justify-end gap-3 max-[899px]:right-2 max-[899px]:bottom-2">
      {checked && layout.pills.length > 0 ? (
        <div inert={layout.full !== null} className="flex max-w-[50vw] flex-wrap-reverse items-end justify-end gap-2 max-[899px]:max-w-none">
          {layout.pills.map((id) => (
            <DockPill key={id} draftId={id} onOpen={() => store.open(id, { animate: true })} onClose={() => store.close(id)} />
          ))}
        </div>
      ) : null}
      {/* first opened on the right; newer windows open to its left */}
      <div className="flex flex-row-reverse items-end gap-3">
        {state.windows
          .filter((entry) => kept.has(entry.draftId) || (checked && visible.has(entry.draftId)))
          .map((entry) => (
            <DockWindowFrame
              key={entry.draftId}
              entry={entry}
              visible={visible.has(entry.draftId)}
              behindModal={layout.full !== null && layout.full !== entry.draftId}
              store={store}
              autoFocus={entry.openedAt > mountSequence}
            />
          ))}
      </div>
    </div>
  )
}

/** A window's name: its subject, else what kind of message it is. */
function draftTitle(draft: { subject: string; mode: string } | undefined): string {
  if (draft?.subject.trim()) return draft.subject.trim()
  if (!draft || draft.mode === 'new') return 'New message'
  return draft.mode.startsWith('forward') ? 'Forward' : 'Reply'
}

function DockWindowFrame({
  entry,
  visible,
  behindModal,
  store,
  autoFocus,
}: {
  entry: DockWindow
  visible: boolean
  /** Another window is full screen: this one is inert. */
  behindModal: boolean
  store: ComposerDockStore
  autoFocus: boolean
}) {
  const { draftId, state, openedAt, animate } = entry
  const full = state === 'full'
  const title = draftTitle(useDraft(draftId).data)
  const section = useRef<HTMLElement>(null)
  const handle = useRef<ComposerHandle>(null)
  const announce = useAnnounceSent()
  const archiveAfterSend = useArchiveAfterSend()

  // Where the focus was before the window showed: it goes back there when the window is minimized or closed.
  const returnFocus = useRef<HTMLElement | null>(null)
  // Brought to the front again (a pill, the reply key, full screen and back): the focus goes in. The first open
  // focuses by itself (autoFocus).
  const seen = useRef(openedAt)
  useLayoutEffect(() => {
    if (!visible) return
    const active = document.activeElement
    if (active instanceof HTMLElement && active !== document.body && !section.current?.contains(active)) returnFocus.current = active
    if (seen.current === openedAt) return
    seen.current = openedAt
    handle.current?.focus()
  }, [visible, openedAt])
  // Full screen is modal: the app beside the dock is inert, so the focus stays in (the other windows and pills get
  // `inert` from the dock). The Sonner toaster and portaled menus are outside the shell and stay usable. Declared after
  // the effect that notes where the focus was (before the page goes inert) and before the ones that give it back
  // (on close its cleanup runs first, so the focus can go back to the page).
  useLayoutEffect(() => {
    const dock = section.current?.closest('[data-slot=composer-dock]')
    const shell = dock?.parentElement
    if (!full || !visible || !dock || !shell) return
    const made: Element[] = []
    const cover = (element: Element) => {
      if (element === dock || element.hasAttribute('inert')) return
      element.setAttribute('inert', '')
      made.push(element)
    }
    for (const element of shell.children) cover(element)
    // and what the shell shows while full screen lasts
    const observer = new MutationObserver((records) => {
      for (const record of records) for (const node of record.addedNodes) if (node instanceof Element) cover(node)
    })
    observer.observe(shell, { childList: true })
    return () => {
      observer.disconnect()
      for (const element of made) element.removeAttribute('inert')
    }
  }, [full, visible])

  const giveBackFocus = useRef(() => {
    const target = returnFocus.current
    returnFocus.current = null
    const active = document.activeElement
    // only when the focus was in this window (or got lost with it), never away from another window
    if (target?.isConnected && (!active || active === document.body || section.current?.contains(active))) target.focus({ preventScroll: true })
  })
  useLayoutEffect(() => {
    if (!visible) giveBackFocus.current()
  }, [visible])
  useLayoutEffect(() => {
    const giveBack = giveBackFocus.current
    return () => giveBack()
  }, [])

  const onSent: SentHandler = (outbound, draft, { archive }) => {
    // ⌘⇧Enter: archived (the next thread opens when the mail page shows this one); Undo puts it back
    const undoArchive = archive && draft.thread_id ? archiveAfterSend(draft.thread_id) : undefined
    announce(outbound, (restored) => {
      undoArchive?.()
      reopenDraft(restored, store, 'window')
    })
  }

  return (
    <section
      ref={section}
      role="dialog"
      // a window sits beside the page (the page keeps its keys); full screen is modal
      aria-modal={full}
      aria-label={`Draft: ${title}`}
      hidden={!visible}
      inert={behindModal}
      data-slot="composer-window"
      data-state={state}
      className={cn(
        'pointer-events-auto flex flex-col',
        full
          ? 'fixed inset-0 z-50'
          : 'h-[min(620px,calc(100svh-2rem))] w-[520px] origin-bottom-right max-[899px]:fixed max-[899px]:inset-2 max-[899px]:h-auto max-[899px]:w-auto',
        // opened by a click: 200 ms from the bottom right (reduced motion: the fade only). Opened by a key, and full
        // screen and back: at once. A hidden window shown again by a click plays it again (display: none restarts it).
        !full && animate && 'animate-composer-window-enter motion-reduce:animate-composer-window-fade',
        !visible && 'hidden',
      )}
    >
      <Composer
        draftId={draftId}
        placement={full ? 'full' : 'window'}
        className="min-h-0 flex-1"
        handleRef={handle}
        autoFocus={autoFocus}
        onClose={() => store.close(draftId)}
        onSent={onSent}
        // Esc: a window goes to a pill, full screen goes back to a window
        onEscape={() => (full ? store.open(draftId) : store.minimize(draftId))}
        onPopOut={() => (full ? store.open(draftId) : store.expand(draftId))}
        headerActions={
          <>
            <Tip
              label={
                <span className="flex items-center gap-2">
                  Minimize <ShortcutKeys keys="Escape" />
                </span>
              }
            >
              <Button variant="ghost" size="icon-sm" className="text-muted-foreground" aria-label="Minimize" onClick={() => store.minimize(draftId)}>
                <Minus />
              </Button>
            </Tip>
            <Tip
              label={
                <span className="flex items-center gap-2">
                  {full ? 'Exit full screen' : 'Full screen'} <ShortcutKeys keys="Mod+Shift+E" />
                </span>
              }
            >
              <Button
                variant="ghost"
                size="icon-sm"
                className="text-muted-foreground max-[899px]:hidden"
                aria-label={full ? 'Exit full screen' : 'Full screen'}
                onClick={() => (full ? store.open(draftId) : store.expand(draftId))}
              >
                {full ? <Minimize /> : <Maximize4 />}
              </Button>
            </Tip>
          </>
        }
      />
    </section>
  )
}

/** A minimized window: its subject; a click opens it again. No motion (pills come and go often). */
function DockPill({ draftId, onOpen, onClose }: { draftId: string; onOpen: () => void; onClose: () => void }) {
  const title = draftTitle(useDraft(draftId).data)
  return (
    <div data-slot="composer-pill" className="pointer-events-auto flex h-9 max-w-60 min-w-36 items-center rounded-lg border bg-popover text-[0.8125rem] text-popover-foreground shadow-lg">
      <button
        type="button"
        className="flex h-full min-w-0 flex-1 items-center gap-2 rounded-l-lg pr-1 pl-3 text-left outline-none hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50"
        aria-label={`Open draft: ${title}`}
        onClick={onOpen}
      >
        <Edit className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate font-medium">{title}</span>
      </button>
      <Tip label="Close (the draft is kept)">
        <Button variant="ghost" size="icon-xs" className="mr-1 text-muted-foreground" aria-label={`Close draft: ${title}`} onClick={onClose}>
          <X />
        </Button>
      </Tip>
    </div>
  )
}

/** The dock crashed: the page keeps working; the drafts are on the server. */
export function ComposerDockError() {
  return (
    <div role="alert" className="fixed right-4 bottom-4 z-40 flex items-center gap-3 rounded-lg border bg-popover py-2 pr-2 pl-4 text-[0.8125rem] text-popover-foreground shadow-lg">
      The mail composer stopped working. Your drafts are saved.
      <Button size="sm" variant="outline" onClick={() => window.location.reload()}>
        Reload
      </Button>
    </div>
  )
}
