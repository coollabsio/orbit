import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Navigate, useNavigate, useParams, useSearchParams } from 'react-router'
import { toast } from 'sonner'
import { Archive, DirectInbox, SearchNormal, ShieldCross, Sms as Mail, Star, Tag, Trash, Undo, Xmark } from 'reicon-react'
import type { MailFolder, MailThreadSummary } from '@orbit/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { PaneHeader } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { useBulkUpdateThreads, useUpdateThread, type ThreadChange } from '@/features/mail/api/mutations'
import { useDrafts, useMailCounts, useMailMe, useMailSearch, useOutbound, useThread, useThreads } from '@/features/mail/api/queries'
import { FolderRail } from '@/features/mail/components/FolderRail'
import { MailEmptyState } from '@/features/mail/components/MailEmptyState'
import { MailIdlePane } from '@/features/mail/components/MailIdlePane'
import { PausedBanner, VacationBanners } from '@/features/mail/components/MailBanners'
import { MailboxNotice } from '@/features/mail/components/MailboxNotice'
import { MailSearch } from '@/features/mail/components/MailSearch'
import { DraftList, OutboundList } from '@/features/mail/components/SendLists'
import { ThreadList } from '@/features/mail/components/ThreadList'
import { LabelMenu, ThreadView, type LabelState } from '@/features/mail/components/ThreadView'
import { useArchiveNextHost, useComposerDock, useNewEmail } from '@/features/mail/compose/useComposerDock'
import type { ReplyMode, ThreadReplyHandle } from '@/features/mail/compose/ThreadReply'
import {
  FOLDERS,
  folderLabel,
  type ListFolder,
  mailThreadPath,
  mailViewKey,
  mailViewPath,
  parseMailPath,
  problemText,
  threadFilterOf,
  withKept,
  type MailView,
} from '@/features/mail/mailLib'
import { useCommand } from '@/shortcuts/useCommand'

/** `POST threads/bulk` takes at most this many ids. */
const BULK_LIMIT = 100

const EMPTY_FOLDER: Record<ListFolder, { title: string; description: string }> = {
  inbox: { title: 'You are all caught up', description: 'New mail shows up here.' },
  archive: { title: 'Nothing archived', description: 'Archived conversations show up here. Press E to archive one.' },
  spam: { title: 'No spam', description: 'Mail that fails authentication lands here.' },
  trash: { title: 'Trash is empty', description: 'Conversations in the trash are deleted for good after 30 days.' },
  drafts: { title: 'No drafts', description: 'Drafts you start show up here.' },
  sent: { title: 'Nothing sent', description: 'Mail you send shows up here.' },
  scheduled: { title: 'Nothing scheduled', description: 'Mail you schedule shows up here until it goes.' },
  outbox: { title: 'Outbox is empty', description: 'Mail that is waiting to go shows up here.' },
}

/** The reading pane of the Drafts, Scheduled and Outbox folders, which open no thread. */
const SEND_PANE: Record<'drafts' | 'scheduled' | 'outbox', { title: string; description: string }> = {
  drafts: { title: 'Select a draft', description: 'It opens in a window, where you keep writing.' },
  scheduled: { title: 'Scheduled mail', description: 'Each message goes at its time. Cancel send brings it back as a draft.' },
  outbox: { title: 'Outbox', description: 'Messages wait here during their undo time, over a send limit, or when something stops them.' },
}

/** `/mail/*`: the mail client (spec §8.3, §9.1). */
export function MailPage() {
  const route = parseMailPath(useParams()['*'] ?? '')
  if (!route) return <Navigate to="/mail" replace />
  // a new view starts with no selection and no focused row
  return <MailApp key={mailViewKey(route.view)} view={route.view} threadId={route.threadId} />
}

function MailApp({ view, threadId }: { view: MailView; threadId: string | null }) {
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const query = view.kind === 'search' ? (searchParams.get('q') ?? '') : ''
  const me = useMailMe()
  const counts = useMailCounts()
  const filter = threadFilterOf(view)
  // **Unread**: the server lists only unread conversations; those opened since stay (as read) until the view changes
  const [unreadOnly, setUnreadOnly] = useState(false)
  const [kept, setKept] = useState<ReadonlyMap<string, MailThreadSummary>>(() => new Map())
  const list = useThreads(filter && unreadOnly ? { ...filter, unread: true } : (filter ?? {}), filter !== null)
  const search = useMailSearch(query)
  // Drafts, Scheduled and Outbox read the drafts and the outbound queue; every list marks the threads with a draft
  const sendView = view.kind === 'folder' && (view.folder === 'drafts' || view.folder === 'scheduled' || view.folder === 'outbox') ? view.folder : null
  const source = view.kind === 'search' ? search : list
  // a send folder has no threads (the idle list query may hold another folder's cached rows)
  const loaded = sendView ? [] : (source.data ?? [])
  const threads = unreadOnly ? withKept(loaded, kept, view.kind === 'folder' && view.folder === 'sent') : loaded
  const detail = useThread(threadId)
  // with a mailbox open, the send folders are that mailbox's
  const scope = view.kind === 'folder' ? view.mailboxId : undefined
  const drafts = useDrafts()
  const scheduled = useOutbound('scheduled', scope, sendView === 'scheduled')
  const outbox = useOutbound('outbox', scope)
  // last changed first
  const scopedDrafts = (drafts.data ?? []).filter((draft) => !scope || draft.mailbox_id === scope).sort((a, b) => b.updated_at.localeCompare(a.updated_at))
  const outboundItems = (sendView === 'scheduled' ? scheduled.data : outbox.data) ?? []
  // the rows j / k move through in a send folder (`null`: the thread list's)
  const sendRowIds = sendView === 'drafts' ? scopedDrafts.map((draft) => draft.id) : sendView ? outboundItems.map((item) => item.id) : null
  const { store: dock } = useComposerDock()
  const update = useUpdateThread()
  const bulk = useBulkUpdateThreads()
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set())
  const [focusedId, setFocusedId] = useState<string | null>(threadId)
  const [labelMenu, setLabelMenu] = useState<'thread' | 'bulk' | null>(null)
  const searchInput = useRef<HTMLInputElement>(null)
  const replyRef = useRef<ThreadReplyHandle>(null)
  // R / A / F on a row of the list: the thread opens, then its reply (`from`: the thread open when the key was pressed)
  const [pendingReply, setPendingReply] = useState<{ threadId: string; from: string | null; mode: ReplyMode } | null>(null)
  const newEmail = useNewEmail()

  const labels = me.data?.labels ?? []
  const labelsById = new Map(labels.map((label) => [label.id, label]))
  const mailboxes = me.data?.mailboxes ?? []
  const draftThreads = new Set((drafts.data ?? []).flatMap((draft) => (draft.thread_id ? [draft.thread_id] : [])))
  // the Outbox shows while something waits or failed, and while it is the open folder
  const showOutbox = (outbox.data?.length ?? 0) > 0 || sendView === 'outbox'
  const openThread = detail.data?.thread ?? threads.find((thread) => thread.id === threadId)

  const pathOf = (id: string | null) => {
    const path = id ? mailThreadPath(view, id) : mailViewPath(view)
    return view.kind === 'search' && query ? `${path}?q=${encodeURIComponent(query)}` : path
  }
  const open = (id: string) => {
    setFocusedId(id)
    const row = unreadOnly ? threads.find((thread) => thread.id === id) : undefined
    if (row) setKept((previous) => new Map(previous).set(id, { ...row, unread: false }))
    navigate(pathOf(id))
  }
  const close = () => navigate(pathOf(null))

  // Opening a thread reads it; a newer message read again later. "Mark as unread" keeps it unread while it stays open.
  const readKey = useRef<string | null>(null)
  const shown = detail.data?.thread
  useEffect(() => {
    if (!threadId) readKey.current = null
  }, [threadId])
  useEffect(() => {
    if (!shown || shown.id !== threadId) return
    const key = `${shown.id}:${shown.last_message_at}`
    if (readKey.current === key) return
    readKey.current = key
    if (shown.unread) update.mutate({ threadId: shown.id, change: { read: true } })
  }, [shown, threadId, update])

  // what an action works on: the selection, else the open thread, else the focused row
  const targets = (): MailThreadSummary[] => {
    const ids = selected.size > 0 ? [...selected] : threadId ? [threadId] : focusedId ? [focusedId] : []
    return ids.flatMap((id) => threads.find((thread) => thread.id === id) ?? (openThread?.id === id ? [openThread] : []))
  }

  const apply = (items: MailThreadSummary[], change: ThreadChange, leaves: boolean) => {
    if (items.length === 0) return
    const onError = (error: Error) => toast.error(problemText(error, 'The change could not be saved.'))
    const ids = items.map((thread) => thread.id)
    if (ids.length === 1) update.mutate({ threadId: ids[0], change }, { onError })
    else for (let start = 0; start < ids.length; start += BULK_LIMIT) bulk.mutate({ threadIds: ids.slice(start, start + BULK_LIMIT), change }, { onError })
    if (selected.size > 0) setSelected(new Set())
    if (!leaves) return
    // the focus moves on to the row after the ones that left
    if (focusedId && ids.includes(focusedId)) {
      const last = Math.max(...ids.map((id) => threads.findIndex((thread) => thread.id === id)))
      setFocusedId(threads.slice(last + 1).find((thread) => !ids.includes(thread.id))?.id ?? null)
    }
    // the open thread left this view (or was marked unread): back to the list
    if (threadId && ids.includes(threadId)) close()
  }
  const all = (items: MailThreadSummary[], folder: MailFolder) => items.length > 0 && items.every((thread) => thread.folder === folder)

  const archive = (items = targets()) => apply(items, { folder: all(items, 'archive') ? 'inbox' : 'archive' }, true)
  const spam = (items = targets()) => apply(items, { folder: all(items, 'spam') ? 'inbox' : 'spam' }, true)
  const trash = (items = targets()) => {
    const kept = items.filter((thread) => thread.folder !== 'trash')
    apply(kept, { folder: 'trash' }, true)
  }
  const restore = (items = targets()) => apply(items.filter((thread) => thread.folder === 'trash'), { restore: true }, true)
  const star = (items = targets()) => apply(items, { starred: !items.every((thread) => thread.starred) }, view.kind === 'starred')
  const toggleRead = (items = targets()) => {
    const read = items.some((thread) => thread.unread)
    apply(items, { read }, !read)
  }
  const labelState = (items: MailThreadSummary[], labelId: string): LabelState => {
    const count = items.filter((thread) => thread.labels.includes(labelId)).length
    return count === 0 ? 'none' : count === items.length ? 'all' : 'some'
  }
  const toggleLabel = (items: MailThreadSummary[], labelId: string) => {
    const remove = labelState(items, labelId) === 'all'
    apply(items, remove ? { remove_labels: [labelId] } : { add_labels: [labelId] }, remove && view.kind === 'label' && view.labelId === labelId)
  }

  const toggleSelect = (id: string) =>
    setSelected((previous) => {
      const next = new Set(previous)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const move = (step: 1 | -1) => {
    if (sendRowIds) {
      if (sendRowIds.length === 0) return
      const current = focusedId ? sendRowIds.indexOf(focusedId) : -1
      setFocusedId(sendRowIds[current < 0 ? 0 : Math.min(sendRowIds.length - 1, Math.max(0, current + step))])
      return
    }
    if (threads.length === 0) return
    const current = threads.findIndex((thread) => thread.id === (focusedId ?? threadId))
    const index = current < 0 ? 0 : Math.min(threads.length - 1, Math.max(0, current + step))
    const next = threads[index].id
    // with a thread open, j / k read the next one
    if (threadId) open(next)
    else setFocusedId(next)
  }
  const hasTarget = () => targets().length > 0
  // Enter on a control (a row, a button, a link) is that control's own
  const focusOnControl = () => document.activeElement?.closest('button, a, [role="button"], [role="option"]') != null

  useCommand('mail.down', () => move(1))
  useCommand('mail.up', () => move(-1))
  // in Drafts, o / Enter continue the focused draft (a key: the window opens without motion); Scheduled and Outbox
  // rows open nothing
  const focusedDraft = sendView === 'drafts' ? scopedDrafts.find((draft) => draft.id === focusedId) : undefined
  const openFocused = () => {
    if (focusedDraft) dock.open(focusedDraft.id)
    else if (!sendView && focusedId) open(focusedId)
  }
  const canOpenFocused = () => (sendView ? focusedDraft !== undefined : focusedId !== null)
  useCommand('mail.open', openFocused, { available: canOpenFocused })
  useCommand('mail.openEnter', openFocused, { available: () => canOpenFocused() && focusedId !== threadId && !focusOnControl() })
  useCommand(
    'mail.close',
    () => {
      if (selected.size > 0) setSelected(new Set())
      else close()
    },
    { available: () => selected.size > 0 || threadId !== null },
  )
  useCommand('mail.archive', () => archive(), { available: hasTarget })
  useCommand('mail.trash', () => trash(), { available: hasTarget })
  useCommand('mail.spam', () => spam(), { available: hasTarget })
  useCommand('mail.star', () => star(), { available: hasTarget })
  useCommand('mail.read', () => toggleRead(), { available: hasTarget })
  useCommand('mail.label', () => setLabelMenu(selected.size > 0 ? 'bulk' : 'thread'), {
    available: () => selected.size > 0 || (threadId !== null && detail.data !== undefined),
  })
  useCommand('mail.select', () => {
    const id = focusedId ?? threadId
    if (id) toggleSelect(id)
  }, { available: () => !sendView && (focusedId ?? threadId) !== null })
  useCommand('mail.search', () => searchInput.current?.focus())
  useCommand('mail.compose', () => void newEmail(), { enabled: mailboxes.length > 0 })
  const reply = (mode: ReplyMode) => {
    if (threadId) {
      replyRef.current?.reply(mode)
    } else if (focusedId) {
      setPendingReply({ threadId: focusedId, from: threadId, mode })
      open(focusedId)
    }
  }
  // with a thread open its messages must be there; in the list, a row must have the focus
  const canReply = () => (threadId ? detail.data !== undefined : !sendView && focusedId !== null)
  useCommand('mail.reply', () => reply('reply'), { available: canReply })
  useCommand('mail.replyAll', () => reply('reply_all'), { available: canReply })
  useCommand('mail.forward', () => reply('forward'), { available: canReply })
  // the user went elsewhere before the thread opened: the reply is dropped
  if (pendingReply && threadId !== pendingReply.threadId && threadId !== pendingReply.from) setPendingReply(null)
  useEffect(() => {
    if (!pendingReply || threadId !== pendingReply.threadId || !detail.data || !replyRef.current) return
    replyRef.current.reply(pendingReply.mode)
    setPendingReply(null)
  }, [pendingReply, threadId, detail.data])

  // ⌘⇧Enter on a reply to the open thread: it is archived and the next one opens (the one
  // below, else above; else the list). Registered for the open thread only: a send that ends after the page moved on
  // only archives.
  useArchiveNextHost(threadId, () => {
    if (!threadId) return
    const index = threads.findIndex((thread) => thread.id === threadId)
    const next = index < 0 ? undefined : (threads[index + 1] ?? threads[index - 1])
    if (openThread) apply([openThread], { folder: 'archive' }, false)
    else update.mutate({ threadId, change: { folder: 'archive' } }, { onError: (error) => toast.error(problemText(error, 'The change could not be saved.')) })
    if (next) open(next.id)
    else close()
  })
  useCommand('mail.starred', () => navigate(mailViewPath({ kind: 'starred' })))
  useCommand('mail.trashFolder', () => navigate(mailViewPath({ kind: 'folder', folder: 'trash' })))

  const mailboxName = (id: string | undefined) => mailboxes.find((mailbox) => mailbox.id === id)?.name
  const title =
    view.kind === 'folder'
      ? view.mailboxId
        ? `${mailboxName(view.mailboxId) ?? 'Mailbox'} · ${folderLabel(view.folder)}`
        : folderLabel(view.folder)
      : view.kind === 'label'
        ? (labelsById.get(view.labelId)?.name ?? 'Label')
        : view.kind === 'starred'
          ? 'Starred'
          : 'Search'
  const icon = view.kind === 'folder' ? (FOLDERS.find((entry) => entry.folder === view.folder)?.icon ?? Mail) : view.kind === 'label' ? Tag : view.kind === 'starred' ? Star : SearchNormal
  const empty = unreadOnly
    ? { title: 'No unread conversations', description: 'You have read everything here.' }
    : view.kind === 'folder'
      ? EMPTY_FOLDER[view.folder]
      : view.kind === 'label'
        ? { title: 'No conversations with this label', description: 'Press L in a conversation to add a label.' }
        : view.kind === 'starred'
          ? { title: 'No starred conversations', description: 'Press S to star a conversation.' }
          : query
            ? { title: 'No results', description: 'Try other words, or fewer operators.' }
            : { title: 'Search your mail', description: 'Use from:, to:, subject:, label:, in:, is:unread, has:attachment, before: and after:.' }

  // no mailbox to read: an admin sets mail up, a member asks for a mailbox
  if (me.data && mailboxes.length === 0) {
    return (
      <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
        {me.data.can_manage ? (
          <MailEmptyState
            icon={Mail}
            title="Set up mail"
            description="Add a domain and create mailboxes, and mail for your team arrives here."
            action={<Button onClick={() => navigate('/settings/mail/domains')}>Set up mail</Button>}
          />
        ) : (
          <MailEmptyState icon={Mail} title="No mailbox yet" description="Ask an admin for a mailbox." />
        )}
      </div>
    )
  }

  const inScope = scope ? mailboxes.filter((mailbox) => mailbox.id === scope) : mailboxes
  const sendSource = sendView === 'drafts' ? drafts : sendView === 'scheduled' ? scheduled : sendView === 'outbox' ? outbox : null
  const sendRows =
    sendView === 'drafts'
      ? { count: scopedDrafts.length, content: <DraftList drafts={scopedDrafts} focusedId={focusedId} onFocus={setFocusedId} /> }
      : sendView
        ? { count: outboundItems.length, content: <OutboundList view={sendView} items={outboundItems} focusedId={focusedId} onFocus={setFocusedId} /> }
        : undefined
  // the unread count next to the title: the folder's (Inbox, Spam) from the counts
  const unreadCount =
    view.kind === 'folder' && (view.folder === 'inbox' || view.folder === 'spam')
      ? (counts.data?.mailboxes ?? []).filter((mailbox) => !view.mailboxId || mailbox.mailbox_id === view.mailboxId).reduce((sum, mailbox) => sum + mailbox[view.folder as 'inbox' | 'spam'], 0)
      : 0
  // a list of several mailboxes names each row's mailbox
  const mixed = mailboxes.length > 1 && !(view.kind === 'folder' && view.mailboxId)
  const people = [...new Set(threads.flatMap((thread) => thread.from.map((person) => person.address)))]
  const picked = threads.filter((thread) => selected.has(thread.id))
  const bulkBar =
    selected.size > 0 ? (
      <PaneHeader data-slot="mail-bulk-bar">
        <Tip label="Clear the selection" side="bottom">
          <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Clear the selection" onClick={() => setSelected(new Set())}>
            <Xmark className="size-4" />
          </Button>
        </Tip>
        <span className="text-[0.8125rem] font-semibold">{selected.size} selected</span>
        <span className="flex-1" />
        <BulkButton label={all(picked, 'archive') ? 'Move to inbox' : 'Archive'} onClick={() => archive(picked)}>
          {all(picked, 'archive') ? <DirectInbox className="size-4" /> : <Archive className="size-4" />}
        </BulkButton>
        <BulkButton label={all(picked, 'spam') ? 'Not spam' : 'Report spam'} onClick={() => spam(picked)}>
          <ShieldCross className="size-4" />
        </BulkButton>
        {all(picked, 'trash') ? (
          <BulkButton label="Restore" onClick={() => restore(picked)}>
            <Undo className="size-4" />
          </BulkButton>
        ) : (
          <BulkButton label="Move to trash" onClick={() => trash(picked)}>
            <Trash className="size-4" />
          </BulkButton>
        )}
        <BulkButton label={picked.some((thread) => thread.unread) ? 'Mark as read' : 'Mark as unread'} onClick={() => toggleRead(picked)}>
          <Mail className="size-4" />
        </BulkButton>
        <BulkButton label="Star" onClick={() => star(picked)}>
          <Star className="size-4" />
        </BulkButton>
        <LabelMenu
          labels={labels}
          state={(id) => labelState(picked, id)}
          onToggle={(id) => toggleLabel(picked, id)}
          open={labelMenu === 'bulk'}
          onOpenChange={(next) => setLabelMenu(next ? 'bulk' : null)}
          canManage={me.data?.can_manage ?? false}
        />
      </PaneHeader>
    ) : null

  const threadActions = {
    star: () => openThread && star([openThread]),
    markUnread: () => openThread && apply([openThread], { read: false }, true),
    archive: () => openThread && archive([openThread]),
    spam: () => openThread && spam([openThread]),
    trash: () => openThread && trash([openThread]),
    restore: () => openThread && restore([openThread]),
    toggleLabel: (labelId: string) => openThread && toggleLabel([openThread], labelId),
  }

  return (
    <div className="group/mail flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background" data-view={threadId ? 'thread' : 'list'}>
      <FolderRail view={view} mailboxes={mailboxes} labels={labels} counts={counts.data} showOutbox={showOutbox} onCompose={mailboxes.length > 0 ? () => void newEmail() : undefined} />
      <ThreadList
        title={title}
        view={view}
        threads={threads}
        labels={labelsById}
        status={view.kind === 'search' && !query ? 'success' : (sendSource ?? source).status}
        onRetry={() => void (sendSource ?? source).refetch()}
        hasMore={!sendView && source.hasNextPage}
        loadingMore={source.isFetchingNextPage}
        onLoadMore={() => void source.fetchNextPage()}
        activeThreadId={threadId}
        focusedId={focusedId}
        selected={selected}
        empty={{ icon, ...empty }}
        search={
          <MailSearch
            key={query}
            defaultQuery={query}
            labels={labels.map((label) => label.name)}
            people={people}
            inputRef={searchInput}
            onSearch={(next) => navigate(next ? `/mail/search?q=${encodeURIComponent(next)}` : '/mail')}
          />
        }
        bulkBar={bulkBar}
        notice={
          <div data-slot="mail-notices" className="flex flex-col gap-3 p-3 empty:hidden">
            <PausedBanner />
            <VacationBanners mailboxes={inScope} canManage={me.data?.can_manage ?? false} />
            <MailboxNotice mailboxes={inScope} canManage={me.data?.can_manage ?? false} />
          </div>
        }
        draftThreads={draftThreads}
        showOutbox={showOutbox}
        rows={sendRows}
        unreadCount={unreadCount}
        unreadOnly={!sendView && view.kind !== 'search' ? { value: unreadOnly, onChange: (next) => { setUnreadOnly(next); setKept(new Map()) } } : undefined}
        onMarkAllRead={!sendView ? () => apply(loaded.filter((thread) => thread.unread), { read: true }, false) : undefined}
        mailboxName={mixed ? (thread) => mailboxName(thread.mailbox_id) : undefined}
        onOpen={open}
        onSelect={toggleSelect}
        onStar={(thread) => star([thread])}
        onArchive={(thread) => archive([thread])}
        onToggleRead={(thread) => toggleRead([thread])}
        onTrash={(thread) => trash([thread])}
        onSwipe={(thread, step) => {
          if ('change' in step.outcome) apply([thread], step.outcome.change, step.leaves)
          else {
            setPendingReply({ threadId: thread.id, from: threadId, mode: step.outcome.reply })
            open(thread.id)
          }
        }}
      />
      <section className="flex h-full min-h-0 min-w-0 flex-1 flex-col border-l bg-background max-[899px]:border-l-0 max-[899px]:group-data-[view=list]/mail:hidden">
        {threadId ? (
          <ThreadView
            key={threadId}
            detail={detail.data}
            status={detail.status}
            onRetry={() => void detail.refetch()}
            labels={labels}
            labelsById={labelsById}
            canManage={me.data?.can_manage ?? false}
            onBack={close}
            actions={threadActions}
            labelMenuOpen={labelMenu === 'thread'}
            onLabelMenuOpenChange={(next) => setLabelMenu(next ? 'thread' : null)}
            replyRef={replyRef}
          />
        ) : (
          <div className="flex min-h-0 flex-1 flex-col">
            {sendView ? (
              <MailEmptyState icon={Mail} {...SEND_PANE[sendView]} />
            ) : (
              <MailIdlePane unread={unreadCount} inbox={view.kind === 'folder' && view.folder === 'inbox'} />
            )}
          </div>
        )}
      </section>
    </div>
  )
}

function BulkButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <Tip label={label} side="bottom">
      <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label={label} onClick={onClick}>
        {children}
      </Button>
    </Tip>
  )
}
