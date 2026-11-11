import { useMemo, useState } from 'react'
import { ChevronDown, ChevronRight, Folder } from 'reicon-react'
import type { GithubPullFile } from '@orbit/api/generated/types.gen'
import { cn } from 'cn'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { buildFileTree, type TreeNode } from '@/features/pulls/lib/fileTree'

/** The changed files as a tree, with +/- counts and a "viewed" tick for each file. */
export function PullFileTree({ files, active, onOpen, onViewed }: {
  files: GithubPullFile[]
  active: string | null
  onOpen: (path: string) => void
  onViewed: (path: string, viewed: boolean) => void
}) {
  const [filter, setFilter] = useState('')
  const shown = useMemo(() => {
    const words = filter.trim().toLowerCase()
    return buildFileTree(words ? files.filter((file) => file.path.toLowerCase().includes(words)) : files)
  }, [files, filter])
  const viewed = files.filter((file) => file.viewed).length
  return <div className="flex min-h-0 flex-col">
    <div className="flex flex-col gap-2 border-b p-2">
      <Input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter files" aria-label="Filter files" className="h-8" />
      <p className="px-1 text-xs text-muted-foreground tabular-nums">{viewed} of {files.length} viewed</p>
    </div>
    <ul role="tree" aria-label="Changed files" className="min-h-0 flex-1 overflow-y-auto p-1">
      {shown.map((node) => <Node key={node.kind === 'folder' ? node.path : node.file.path} node={node} depth={0} active={active} onOpen={onOpen} onViewed={onViewed} />)}
    </ul>
  </div>
}

function Node({ node, depth, active, onOpen, onViewed }: { node: TreeNode; depth: number; active: string | null; onOpen: (path: string) => void; onViewed: (path: string, viewed: boolean) => void }) {
  const [open, setOpen] = useState(true)
  const indent = { paddingLeft: 6 + depth * 12 }
  if (node.kind === 'folder') {
    const Chevron = open ? ChevronDown : ChevronRight
    return <li role="treeitem" aria-expanded={open} aria-selected={false}>
      <button type="button" className="flex w-full items-center gap-1 rounded-md py-1 pr-2 text-left text-xs text-muted-foreground hover:bg-muted" style={indent} onClick={() => setOpen(!open)}>
        <Chevron className="size-3 shrink-0" /><Folder className="size-3.5 shrink-0" /><span className="truncate">{node.name}</span>
      </button>
      {open ? <ul role="group">{node.children.map((child) => <Node key={child.kind === 'folder' ? child.path : child.file.path} node={child} depth={depth + 1} active={active} onOpen={onOpen} onViewed={onViewed} />)}</ul> : null}
    </li>
  }
  const { file } = node
  return <li role="treeitem" data-file={file.path} aria-selected={active === file.path} className={cn('group/file flex items-center gap-1.5 rounded-md py-1 pr-2 text-xs hover:bg-muted', active === file.path && 'bg-muted')} style={indent}>
    <Checkbox checked={file.viewed} onCheckedChange={(checked) => onViewed(file.path, checked === true)} aria-label={`Viewed ${file.path}`} className="size-3.5" />
    <button type="button" className={cn('min-w-0 flex-1 truncate text-left', file.viewed ? 'text-muted-foreground' : 'text-foreground')} title={file.path} onClick={() => onOpen(file.path)}>{node.name}</button>
    <span className="shrink-0 font-mono text-[11px] tabular-nums"><span className="text-emerald-600 dark:text-emerald-400">+{file.additions}</span> <span className="text-red-600 dark:text-red-400">−{file.deletions}</span></span>
  </li>
}
