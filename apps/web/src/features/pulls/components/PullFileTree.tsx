import { useState, type RefObject } from 'react'
import { ChevronDown, ChevronRight, Folder, MessageText } from 'reicon-react'
import type { GithubPullFile, GithubReviewThread } from '@orbit/api/generated/types.gen'
import { asideReason, type AsideReason } from '@orbit/api/github'
import { cn } from 'cn'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { buildFileTree, type TreeNode } from '@/features/pulls/lib/fileTree'

const REASON: Record<AsideReason, string> = { lockfile: 'Lockfile', generated: 'Generated', snapshot: 'Snapshot' }

type Handlers = {
  active: string | null
  threadCount: (path: string) => number
  onOpen: (path: string) => void
  onViewed: (path: string, viewed: boolean) => void
}

/** The files to review as a tree, with a "viewed" tick, +/− and open threads; the files set aside in a closed group. */
export function PullFileTree({ review, aside, threads, active, filterRef, onOpen, onViewed }: {
  review: GithubPullFile[]
  aside: GithubPullFile[]
  threads: GithubReviewThread[]
  active: string | null
  filterRef: RefObject<HTMLInputElement | null>
  onOpen: (path: string) => void
  onViewed: (path: string, viewed: boolean) => void
}) {
  const [filter, setFilter] = useState('')
  const [asideOpen, setAsideOpen] = useState(false)
  const words = filter.trim().toLowerCase()
  const match = (file: GithubPullFile) => !words || file.path.toLowerCase().includes(words)
  const shown = buildFileTree(review.filter(match))
  const asideShown = aside.filter(match)
  const counts = new Map<string, number>()
  for (const thread of threads) if (!thread.is_resolved) counts.set(thread.path, (counts.get(thread.path) ?? 0) + 1)
  const handlers: Handlers = { active, threadCount: (path) => counts.get(path) ?? 0, onOpen, onViewed }
  return <div className="flex min-h-0 flex-1 flex-col">
    <div className="p-2">
      <Input
        ref={filterRef}
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
        onKeyDown={(event) => { if (event.key === 'Escape' && filter) { event.preventDefault(); setFilter('') } }}
        placeholder="Filter files"
        aria-label="Filter files"
        className="h-7 text-xs"
      />
    </div>
    <ul role="tree" aria-label="Changed files" className="min-h-0 flex-1 overflow-y-auto px-1 pb-2">
      {shown.map((node) => <Node key={node.kind === 'folder' ? node.path : node.file.path} node={node} depth={0} {...handlers} />)}
      {shown.length === 0 && asideShown.length === 0 ? <li className="px-2 py-1.5 text-xs text-muted-foreground">No file matches.</li> : null}
      {asideShown.length > 0 ? <li role="treeitem" aria-expanded={asideOpen || words !== ''} aria-selected={false} className="mt-2 border-t pt-2">
        <button type="button" onClick={() => setAsideOpen(!asideOpen)} className="flex w-full items-center gap-1 rounded-md px-1.5 py-1 text-left text-xs text-muted-foreground hover:bg-muted">
          {asideOpen || words ? <ChevronDown className="size-3 shrink-0" /> : <ChevronRight className="size-3 shrink-0" />}
          <span>Set aside</span><span className="tabular-nums">{asideShown.length}</span>
        </button>
        {asideOpen || words ? <ul role="group">{asideShown.map((file) => <FileRow key={file.path} file={file} name={file.path} depth={1} note={REASON[asideReason(file.path) ?? 'generated']} {...handlers} />)}</ul> : null}
      </li> : null}
    </ul>
  </div>
}

function Node({ node, depth, ...handlers }: Handlers & { node: TreeNode; depth: number }) {
  const [open, setOpen] = useState(true)
  if (node.kind === 'file') return <FileRow file={node.file} name={node.name} depth={depth} {...handlers} />
  const Chevron = open ? ChevronDown : ChevronRight
  return <li role="treeitem" aria-expanded={open} aria-selected={false}>
    <button type="button" className="flex w-full items-center gap-1 rounded-md py-1 pr-2 text-left text-xs text-muted-foreground hover:bg-muted" style={{ paddingLeft: 6 + depth * 12 }} onClick={() => setOpen(!open)}>
      <Chevron className="size-3 shrink-0" /><Folder className="size-3.5 shrink-0" /><span className="truncate">{node.name}</span>
    </button>
    {open ? <ul role="group">{node.children.map((child) => <Node key={child.kind === 'folder' ? child.path : child.file.path} node={child} depth={depth + 1} {...handlers} />)}</ul> : null}
  </li>
}

function FileRow({ file, name, depth, note, active, threadCount, onOpen, onViewed }: Handlers & { file: GithubPullFile; name: string; depth: number; note?: string }) {
  const open = threadCount(file.path)
  return <li role="treeitem" data-file={file.path} aria-selected={active === file.path} className={cn('flex items-center gap-1.5 rounded-md py-1 pr-2 text-xs hover:bg-muted', active === file.path && 'bg-muted')} style={{ paddingLeft: 6 + depth * 12 }}>
    <Checkbox checked={file.viewed} onCheckedChange={(checked) => onViewed(file.path, checked === true)} aria-label={`Viewed ${file.path}`} className="size-3.5" />
    <button type="button" className={cn('min-w-0 flex-1 truncate text-left', file.viewed ? 'text-muted-foreground line-through decoration-muted-foreground/40' : 'text-foreground')} title={file.path} onClick={() => onOpen(file.path)}>{name}</button>
    {note ? <span className="shrink-0 text-[11px] text-muted-foreground">{note}</span> : null}
    {open > 0 ? <span className="flex shrink-0 items-center gap-0.5 text-[11px] text-primary tabular-nums" aria-label={`${open} open ${open === 1 ? 'thread' : 'threads'}`}><MessageText className="size-3" />{open}</span> : null}
    <span className="shrink-0 font-mono text-[11px] tabular-nums"><span className="text-emerald-600 dark:text-emerald-400">+{file.additions}</span> <span className="text-red-600 dark:text-red-400">−{file.deletions}</span></span>
  </li>
}
