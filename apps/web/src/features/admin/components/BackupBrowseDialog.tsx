import { useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { ApiProblem } from '@orbit/api/problem'
import type { BackupItem, BackupItemKind, BackupSummary, RestoredItem } from '@orbit/api/generated/types.gen'
import { Modal } from '@/components/common/Modal'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useBackupItems, useBackupWorkspaces, useRestoreBackupItem } from '@/features/admin/api'
import { switchWorkspaceHref } from '@/features/workspaces/navigation'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { relativeTime } from '@/lib/format'
import { useDebouncedValue } from '@/lib/useDebouncedValue'

function failure(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.problem.detail : fallback
}

/** Lists the pages and tasks of one backup and restores one of them as a copy into its live workspace. */
export function BackupBrowseDialog({ backup, onClose }: { backup: BackupSummary; onClose: () => void }) {
  const navigate = useNavigate()
  const workspaces = useBackupWorkspaces(backup.id)
  const [chosen, setChosen] = useState<string | null>(null)
  const [kind, setKind] = useState<BackupItemKind>('page')
  const [query, setQuery] = useState('')
  const search = useDebouncedValue(query.trim(), 250)
  const { workspace: current, workspaces: mine } = useWorkspace()
  const memberOf = new Set(mine.map((workspace) => workspace.id))
  const live = workspaces.data?.filter((workspace) => workspace.live) ?? []
  // The admin's own workspace first: only there can they open the copy.
  const workspaceId = chosen ?? (live.find((workspace) => workspace.id === current.id) ?? live.find((workspace) => memberOf.has(workspace.id)) ?? live[0])?.id ?? null
  const items = useBackupItems(backup.id, workspaceId, kind, search)
  const restore = useRestoreBackupItem(backup.id, workspaceId ?? '')

  function restoreItem(item: BackupItem, includeChildren: boolean) {
    if (!workspaceId) return
    const target = workspaceId
    restore.mutate({ kind: item.kind, id: item.id, include_children: includeChildren }, {
      onSuccess: (restored: RestoredItem) => {
        const url = new URL(restored.path, window.location.origin)
        toast.success(`Restored as ${restored.title}`, {
          description: restored.missing_files > 0 ? `${restored.missing_files} ${restored.missing_files === 1 ? 'file was' : 'files were'} not in the backup.` : undefined,
          action: memberOf.has(target) ? { label: 'Open', onClick: () => navigate(switchWorkspaceHref(url.pathname, url.search, target)) } : undefined,
        })
      },
      onError: (error) => toast.error(failure(error, 'The copy could not be restored.')),
    })
  }

  const names = (workspaces.data ?? []).map((workspace) => workspace.name)
  // Two workspaces can share a name; a short id tells them apart.
  const label = (workspace: { id: string; name: string }) => names.filter((name) => name === workspace.name).length > 1 ? `${workspace.name} · ${workspace.id.slice(-4)}` : workspace.name
  const workspaceItems = (workspaces.data ?? []).map((workspace) => ({ value: workspace.id, label: label(workspace) }))
  const noun = kind === 'page' ? 'pages' : 'tasks'
  return (
    <Modal
      title={`Backup from ${new Date(backup.created_at).toLocaleString()}`}
      description="Restores a copy. Current data does not change. Comments and history are not restored."
      dismissible={!restore.isPending}
      onClose={onClose}
    >
      {workspaces.isPending ? (
        <div role="status" className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
          <Spinner />
          Opening the backup… The first time can take a while.
        </div>
      ) : workspaces.isError ? (
        <div role="alert" className="flex items-center gap-2 py-6 text-sm text-destructive">
          {failure(workspaces.error, 'The backup could not be opened.')}
          <Button variant="ghost" size="sm" onClick={() => void workspaces.refetch()}>Retry</Button>
        </div>
      ) : workspaces.data.length === 0 ? (
        <p className="py-6 text-sm text-muted-foreground">This backup has no workspaces.</p>
      ) : (
        <div className="flex min-w-0 flex-col gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <Select items={workspaceItems} value={workspaceId} onValueChange={(value) => { if (value) setChosen(value) }}>
              <SelectTrigger aria-label="Workspace" className="w-56 max-sm:w-full">
                <SelectValue placeholder="Choose a workspace" />
              </SelectTrigger>
              <SelectContent>
                {workspaces.data.map((workspace) => (
                  <SelectItem key={workspace.id} value={workspace.id} disabled={!workspace.live}>
                    {label(workspace)}
                    {workspace.live ? null : <span className="text-xs text-muted-foreground">Deleted</span>}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Tabs value={kind} onValueChange={(value) => setKind(value as BackupItemKind)}>
              <TabsList>
                <TabsTrigger value="page">Pages</TabsTrigger>
                <TabsTrigger value="task">Tasks</TabsTrigger>
              </TabsList>
            </Tabs>
            <Input type="search" aria-label={`Search ${noun}`} placeholder={`Search ${noun}…`} value={query} onChange={(event) => setQuery(event.target.value)} className="h-8 min-w-40 flex-1" />
          </div>
          {workspaceId === null ? (
            <p className="py-6 text-sm text-muted-foreground">No workspace of this backup still exists, so nothing can be restored.</p>
          ) : items.isPending ? (
            <div role="status" className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><Spinner />Loading {noun}…</div>
          ) : items.isError ? (
            <p role="alert" className="py-6 text-sm text-destructive">{failure(items.error, `The ${noun} could not be loaded.`)}</p>
          ) : items.data.length === 0 ? (
            <p className="py-6 text-sm text-muted-foreground">{search ? `No ${noun} match “${search}”.` : `No ${noun} in this workspace.`}</p>
          ) : (
            <ul aria-label={kind === 'page' ? 'Pages' : 'Tasks'} className="-mx-1 flex max-h-[50vh] flex-col divide-y overflow-y-auto text-[0.8125rem]">
              {items.data.map((item) => (
                <BackupItemRow
                  key={item.id}
                  item={item}
                  restoring={restore.isPending && restore.variables?.id === item.id}
                  disabled={restore.isPending}
                  onRestore={(includeChildren) => restoreItem(item, includeChildren)}
                />
              ))}
            </ul>
          )}
        </div>
      )}
    </Modal>
  )
}

function BackupItemRow({ item, restoring, disabled, onRestore }: { item: BackupItem; restoring: boolean; disabled: boolean; onRestore: (includeChildren: boolean) => void }) {
  const updated = new Date(item.updated_at).toISOString()
  const meta = (item.kind === 'page'
    ? [item.parent_title ? `In ${item.parent_title}` : null, item.child_count > 0 ? `${item.child_count} ${item.child_count === 1 ? 'sub-page' : 'sub-pages'}` : null]
    : [item.project_name, item.number === null ? null : `#${item.number}`]).filter(Boolean).join(' · ')
  return (
    <li className="flex items-center gap-3 px-1 py-2">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex min-w-0 items-center gap-2 font-medium text-foreground">
          <span className="truncate" title={item.title}>{item.title || 'Untitled'}</span>
          {item.deleted ? <Badge variant="secondary">In trash</Badge> : null}
        </span>
        <span className="truncate text-xs text-muted-foreground/70">
          {meta ? `${meta} · ` : null}
          <time dateTime={updated} title={new Date(item.updated_at).toLocaleString()}>{relativeTime(updated)}</time>
        </span>
      </div>
      {item.kind === 'page' && item.child_count > 0
        ? <RestorePageButton title={item.title} restoring={restoring} disabled={disabled} onRestore={onRestore} />
        : (
          <Button variant="outline" size="sm" disabled={disabled} onClick={() => onRestore(false)} aria-label={`Restore a copy of ${item.title}`}>
            {restoring ? 'Restoring…' : 'Restore copy'}
          </Button>
        )}
    </li>
  )
}

/** A page with sub-pages asks first whether to copy them too. */
function RestorePageButton({ title, restoring, disabled, onRestore }: { title: string; restoring: boolean; disabled: boolean; onRestore: (includeChildren: boolean) => void }) {
  const [open, setOpen] = useState(false)
  const [includeChildren, setIncludeChildren] = useState(true)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger render={<Button variant="outline" size="sm" disabled={disabled} aria-label={`Restore a copy of ${title}`} />}>
        {restoring ? 'Restoring…' : 'Restore copy'}
      </PopoverTrigger>
      <PopoverContent align="end" className="w-60">
        <Label className="font-normal">
          <Checkbox checked={includeChildren} onCheckedChange={setIncludeChildren} />
          Include sub-pages
        </Label>
        <Button size="sm" onClick={() => { setOpen(false); onRestore(includeChildren) }}>Restore copy</Button>
      </PopoverContent>
    </Popover>
  )
}
