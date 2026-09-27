import { lazy, Suspense, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { History, Refresh as RotateCcw, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import type { PageVersionSummary } from '@/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { confirmAction } from '@/components/common/confirmAction'
import { Emoji } from '@/components/common/Emoji'
import { EmptyState } from '@/components/common/EmptyState'
import { UserAvatar } from '@/components/common/UserAvatar'
import { dayLabel, timeOfDay } from '@/lib/format'
import { usePageVersion, usePageVersions } from '@/features/docs/api/pageVersions'
// Type-only: erased at build time, so the BlockNote chunk stays lazy.
import type { PageRef } from '@/features/docs/editor/pageEditorContext'

// Same lazy chunk as the page editor: BlockNote loads only when a preview shows.
const PageEditor = lazy(() => import('@/features/docs/editor/PageEditor').then((module) => ({ default: module.PageEditor })))

const CURRENT = 'current'

/** The page as it is now (the live editor state), shown as the "Current version" entry. */
export interface CurrentPageState {
  title: string
  icon: string | null
  content: unknown[]
}

interface PageHistoryPaneProps {
  workspaceId: string
  pageId: string
  /** Read when "Current version" is selected, so it shows unsaved typing too. */
  readCurrent: () => CurrentPageState
  resolvePage: (pageId: string) => PageRef | null
  onOpenPage: (pageId: string) => void
  /** Restores the version onto the open page; resolves to whether it worked (the caller reports errors). */
  onRestore: (version: PageVersionSummary) => Promise<boolean>
  onClose: () => void
}

const KIND_LABEL: Record<PageVersionSummary['kind'], string | null> = {
  auto: null,
  restore: 'Before a restore',
  import: 'Imported from Notion',
}

const noop = () => {}
const noSubpage = () => Promise.reject(new Error('Read-only preview'))
const noPick = () => Promise.resolve(null)

/** Versions grouped by calendar day, newest first (the list is already sorted). */
function groupByDay(versions: PageVersionSummary[]): { label: string; items: PageVersionSummary[] }[] {
  const groups: { label: string; items: PageVersionSummary[] }[] = []
  for (const version of versions) {
    const label = dayLabel(version.created_at)
    const last = groups.at(-1)
    if (last?.label === label) last.items.push(version)
    else groups.push({ label, items: [version] })
  }
  return groups
}

/**
 * Right-hand pane next to the page editor: the page's versions grouped by day (plus the current state on top), a
 * read-only preview of the selected one and "Restore this version". Arrow keys move through the list; Escape closes.
 */
export function PageHistoryPane({ workspaceId, pageId, readCurrent, resolvePage, onOpenPage, onRestore, onClose }: PageHistoryPaneProps) {
  const versions = usePageVersions(workspaceId, pageId)
  const items = useMemo(() => versions.data ?? [], [versions.data])
  const [picked, setPicked] = useState<string | null>(null)
  const [restoring, setRestoring] = useState(false)
  // Until the user picks one, the newest stored version is selected.
  const selectedId = picked ?? items[0]?.id ?? CURRENT
  const selected = items.find((item) => item.id === selectedId) ?? null
  const detail = usePageVersion(workspaceId, pageId, selected?.id ?? null)
  const listRef = useRef<HTMLDivElement>(null)
  const optionIds = useMemo(() => [CURRENT, ...items.map((item) => item.id)], [items])
  const groups = useMemo(() => groupByDay(items), [items])
  // Read once each time "Current version" gets selected, not on every parent render.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const current = useMemo(() => (selectedId === CURRENT ? readCurrent() : null), [selectedId])

  const focusOption = (id: string) => {
    listRef.current?.querySelector<HTMLElement>(`[data-version-id="${id}"]`)?.focus()
  }

  // Focus the selected entry once the list is there, so the keyboard starts inside the pane.
  const listReady = !versions.isPending && items.length > 0
  useEffect(() => {
    if (listReady) focusOption(selectedId)
    // Only when the list first becomes available.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [listReady])

  const onListKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = optionIds.indexOf(selectedId)
    let next: number | null = null
    if (event.key === 'ArrowDown') next = Math.min(optionIds.length - 1, index + 1)
    else if (event.key === 'ArrowUp') next = Math.max(0, index - 1)
    else if (event.key === 'Home') next = 0
    else if (event.key === 'End') next = optionIds.length - 1
    if (next === null) return
    event.preventDefault()
    setPicked(optionIds[next])
    focusOption(optionIds[next])
  }

  const requestRestore = async () => {
    if (!selected) return
    const when = `${dayLabel(selected.created_at)}, ${timeOfDay(selected.created_at)}`
    const confirmed = await confirmAction({
      title: 'Restore this version?',
      description: `The page’s title, icon and content go back to how they were (${when}). The current state stays in the history.`,
      confirmLabel: 'Restore',
    })
    if (!confirmed) return
    setRestoring(true)
    try {
      await onRestore(selected)
    } finally {
      setRestoring(false)
    }
  }

  const preview = selected
    ? detail.data
      ? { title: detail.data.title, icon: detail.data.icon, content: detail.data.content }
      : null
    : current

  let body
  if (versions.isPending) {
    body = (
      <div className="flex flex-1 items-center justify-center">
        <Spinner className="text-muted-foreground" />
      </div>
    )
  } else if (versions.isError) {
    body = (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-sm text-muted-foreground">
        Could not load the version history.
        <Button type="button" variant="outline" size="sm" onClick={() => void versions.refetch()}>
          Retry
        </Button>
      </div>
    )
  } else if (items.length === 0) {
    body = (
      <div className="flex flex-1 flex-col p-6">
        <EmptyState icon={History} title="No versions yet" description="Versions appear after you edit this page." />
      </div>
    )
  } else {
    body = (
      <div className="flex min-h-0 flex-1 max-[899px]:flex-col-reverse">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-4">
            <span className="min-w-0 flex-1 truncate text-[13px] text-muted-foreground" data-testid="history-preview-label">
              {selected ? `${dayLabel(selected.created_at)}, ${timeOfDay(selected.created_at)}` : 'Current version'}
            </span>
            <Button type="button" size="sm" disabled={!selected || !detail.data || restoring} onClick={() => void requestRestore()}>
              {restoring ? <Spinner /> : <RotateCcw className="size-3.5" />}
              Restore this version
            </Button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-6 pt-6 pb-16 max-[899px]:px-3" aria-label="Version preview" role="region">
            {preview ? (
              <div className="mx-auto max-w-[680px]">
                {preview.icon ? (
                  <div className="mb-2 text-[44px] leading-none">
                    <Emoji value={preview.icon} size={44} />
                  </div>
                ) : null}
                <h2 className="mb-4 text-[26px] leading-tight font-bold text-foreground">{preview.title.trim() ? preview.title : 'Untitled'}</h2>
                <Suspense
                  fallback={
                    <div className="flex justify-center py-10">
                      <Spinner className="text-muted-foreground" />
                    </div>
                  }
                >
                  <PageEditor
                    pageId={`history:${selectedId}`}
                    initialContent={preview.content}
                    editable={false}
                    onChange={noop}
                    resolvePage={resolvePage}
                    onOpenPage={onOpenPage}
                    onCreateSubpage={noSubpage}
                    onPickPage={noPick}
                    className="-mx-[54px] max-[899px]:mx-0"
                  />
                </Suspense>
              </div>
            ) : detail.isError ? (
              <p className="py-10 text-center text-sm text-muted-foreground">Could not load this version.</p>
            ) : (
              <div className="flex justify-center py-10">
                <Spinner className="text-muted-foreground" />
              </div>
            )}
          </div>
        </div>
        <div
          ref={listRef}
          role="listbox"
          aria-label="Versions"
          className="flex w-60 shrink-0 flex-col overflow-y-auto border-l border-border p-2 max-[899px]:max-h-[40%] max-[899px]:w-full max-[899px]:border-b max-[899px]:border-l-0"
          onKeyDown={onListKeyDown}
        >
          <VersionOption id={CURRENT} selected={selectedId === CURRENT} onSelect={setPicked} title="Current version" />
          {groups.map((group) => (
            <div key={group.label} role="group" aria-label={group.label} className="mt-2 flex flex-col">
              <div className="px-2 pt-1 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{group.label}</div>
              {group.items.map((version) => (
                <VersionOption
                  key={version.id}
                  id={version.id}
                  selected={selectedId === version.id}
                  onSelect={setPicked}
                  title={timeOfDay(version.created_at)}
                  author={version.created_by?.display_name ?? 'Former member'}
                  note={KIND_LABEL[version.kind]}
                />
              ))}
            </div>
          ))}
          {versions.hasNextPage ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="mt-2 text-muted-foreground"
              disabled={versions.isFetchingNextPage}
              onClick={() => void versions.fetchNextPage()}
            >
              {versions.isFetchingNextPage ? <Spinner /> : null}
              Load older versions
            </Button>
          ) : null}
        </div>
      </div>
    )
  }

  return (
    <aside
      aria-label="Version history"
      className="flex h-full min-h-0 w-[min(720px,50%)] shrink-0 flex-col border-l border-border bg-background max-[899px]:fixed max-[899px]:inset-0 max-[899px]:z-40 max-[899px]:w-full max-[899px]:border-l-0"
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.stopPropagation()
          onClose()
        }
      }}
    >
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
        <History className="size-4 text-muted-foreground" aria-hidden="true" />
        <h2 className="min-w-0 flex-1 truncate text-sm font-semibold text-foreground">Version history</h2>
        <Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Close version history" onClick={onClose}>
          <X className="size-4" />
        </Button>
      </div>
      {body}
    </aside>
  )
}

interface VersionOptionProps {
  id: string
  selected: boolean
  onSelect: (id: string) => void
  title: string
  author?: string
  note?: string | null
}

function VersionOption({ id, selected, onSelect, title, author, note }: VersionOptionProps) {
  return (
    <div
      role="option"
      aria-selected={selected}
      tabIndex={selected ? 0 : -1}
      data-version-id={id}
      className={cn(
        'flex cursor-pointer flex-col gap-0.5 rounded-md px-2 py-1.5 text-left outline-none hover:bg-accent/60 focus-visible:ring-2 focus-visible:ring-ring',
        selected && 'bg-accent hover:bg-accent',
      )}
      onClick={() => onSelect(id)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          onSelect(id)
        }
      }}
    >
      <span className="text-[13px] font-medium text-foreground">{title}</span>
      {author ? (
        <span className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
          <UserAvatar user={null} name={author} size={16} />
          <span className="truncate">{author}</span>
        </span>
      ) : null}
      {note ? <span className="text-[11px] text-muted-foreground/80">{note}</span> : null}
    </div>
  )
}
