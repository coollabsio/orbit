import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Navigate, useNavigate, useParams, useSearchParams } from 'react-router'
import { toast } from 'sonner'
import { Archive, DirectInbox, SearchNormal, ShieldCross, Sms as Mail, Star, Tag, Trash, Undo, Xmark } from 'reicon-react'
import type { MailFolder, MailThreadSummary } from '@orbit/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { PaneHeader } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { useBulkUpdateThreads, useUpdateThread, type ThreadChange } from '@/features/mail/api/mutations'
import { useMailCounts, useMailMe, useMailSearch, useThread, useThreads } from '@/features/mail/api/queries'
import { FolderRail } from '@/features/mail/components/FolderRail'
import { MailEmptyState } from '@/features/mail/components/MailEmptyState'
import { MailboxNotice } from '@/features/mail/components/MailboxNotice'
import { MailSearch } from '@/features/mail/components/MailSearch'
import { ThreadList } from '@/features/mail/components/ThreadList'
import { LabelMenu, ThreadView, type LabelState } from '@/features/mail/components/ThreadView'
import {
  FOLDERS,
  folderLabel,
  mailThreadPath,
  mailViewKey,
  mailViewPath,
  parseMailPath,
  problemText,
  threadFilterOf,
  type MailView,
} from '@/features/mail/mailLib'
import { useCommand } from '@/shortcuts/useCommand'

/** `POST threads/bulk` takes at most this many ids. */
const BULK_LIMIT = 100

const EMPTY_FOLDER: Record<MailFolder, { title: string; description: string }> = {
  inbox: { title: 'Your inbox is empty', description: 'New mail shows up here.' },
  archive: { title: 'Nothing archived', description: 'Archived conversations show up here. Press E to archive one.' },
  spam: { title: 'No spam', description: 'Mail that fails authentication lands here.' },
  trash: { title: 'Trash is empty', description: 'Conversations in the trash are deleted for good after 30 days.' },
}

/** `/mail/*`: the read-only mail client (spec §8.3). */
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
  const list = useThreads(filter ?? {}, filter !== null)
  const search = useMailSearch(query)
  const source = view.kind === 'search' ? search : list
  const threads = source.data ?? []
  const detail = useThread(threadId)
  const update = useUpdateThread()
  const bulk = useBulkUpdateThreads()
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set())
  const [focusedId, setFocusedId] = useState<string | null>(threadId)
  const [labelMenu, setLabelMenu] = useState<'thread' | 'bulk' | null>(null)
  const searchInput = useRef<HTMLInputElement>(null)

  const labels = me.data?.labels ?? []
  const labelsById = new Map(labels.map((label) => [label.id, label]))
  const mailboxes = me.data?.mailboxes ?? []
  const openThread = detail.data?.thread ?? threads.find((thread) => thread.id === threadId)

  const pathOf = (id: string | null) => {
    const path = id ? mailThreadPath(view, id) : mailViewPath(view)
    return view.kind === 'search' && query ? `${path}?q=${encodeURIComponent(query)}` : path
  }
  const open = (id: string) => {
    setFocusedId(id)
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
  useCommand('mail.open', () => focusedId && open(focusedId), { available: () => focusedId !== null })
  useCommand('mail.openEnter', () => focusedId && open(focusedId), { available: () => focusedId !== null && focusedId !== threadId && !focusOnControl() })
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
  }, { available: () => (focusedId ?? threadId) !== null })
  useCommand('mail.search', () => searchInput.current?.focus())
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
  const empty =
    view.kind === 'folder'
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
      <FolderRail view={view} mailboxes={mailboxes} labels={labels} counts={counts.data} />
      <ThreadList
        title={title}
        view={view}
        threads={threads}
        labels={labelsById}
        status={view.kind === 'search' && !query ? 'success' : source.status}
        onRetry={() => void source.refetch()}
        hasMore={source.hasNextPage}
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
          <MailboxNotice
            mailboxes={view.kind === 'folder' && view.mailboxId ? mailboxes.filter((mailbox) => mailbox.id === view.mailboxId) : mailboxes}
            canManage={me.data?.can_manage ?? false}
          />
        }
        onOpen={open}
        onSelect={toggleSelect}
        onStar={(thread) => star([thread])}
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
          />
        ) : (
          <div className="flex min-h-0 flex-1 flex-col">
            <MailEmptyState icon={Mail} title="Select a conversation" description="Choose a conversation from the list to read it here." />
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
