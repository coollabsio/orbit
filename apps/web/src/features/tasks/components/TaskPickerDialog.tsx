import { useMemo, useState } from 'react'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { taskFromRecord, type Task, type TaskStatusDef } from '@/features/tasks/api/models'
import { useProjects } from '@/features/tasks/api/projects'
import { useTasks } from '@/features/tasks/api/tasks'
import { pickerCandidates } from '@/features/tasks/relationsLib'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { TaskStatusIcon } from './TaskStatusIcon'

export interface TaskPickerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Visible heading and the dialog's accessible name, e.g. "Mark ORB-3F2A as duplicate of…". */
  title: string
  statuses: TaskStatusDef[]
  excludeIds?: readonly string[]
  excludeDuplicates?: boolean
  onSelect: (task: Task) => void
}

/** Recently updated tasks: the browse list, and where identifiers match (the server search only knows text). */
const RECENT = { sort: 'updated_at', order: 'desc', limit: 100 } as const

/**
 * Pick another task (duplicate target, blocker, related). Same data pattern as the command palette; the title is
 * rendered inside the popup so it names the dialog. Mount only while open: the queries start on mount.
 */
export function TaskPickerDialog({ open, onOpenChange, title, statuses, excludeIds = [], excludeDuplicates = false, onSelect }: TaskPickerDialogProps) {
  const { workspace } = useWorkspace()
  const projects = useProjects(workspace.id)
  const [query, setQuery] = useState('')
  const search = query.trim()
  const recent = useTasks(workspace.id, RECENT)
  // with an empty query this is the same query key as `recent`, so a single request goes out
  const searched = useTasks(workspace.id, { ...RECENT, search: search || undefined })
  const candidates = useMemo(() => {
    const records = [...(searched.data?.pages ?? []), ...(recent.data?.pages ?? [])].flatMap((page) => page.items)
    const tasks = records.map((record) => taskFromRecord(record, projects.data?.find((project) => project.id === record.project_id)))
    return pickerCandidates({ tasks, query: search, excludeIds, excludeDuplicates, statuses })
  }, [excludeDuplicates, excludeIds, projects.data, recent.data, search, searched.data, statuses])
  const projectName = (projectId: string) => projects.data?.find((project) => project.id === projectId)?.name
  const loading = recent.isPending || searched.isFetching

  return (
    <Dialog open={open} onOpenChange={(next) => onOpenChange(next)}>
      <DialogContent
        showCloseButton={false}
        className="top-[12vh] max-h-[min(60vh,28rem)] translate-y-0 gap-0 overflow-hidden rounded-xl! bg-card p-0 shadow-2xl ring-border sm:max-w-[576px]"
      >
        <DialogTitle className="px-3.5 pt-3 pb-1.5 text-[13px] font-medium text-muted-foreground">{title}</DialogTitle>
        <DialogDescription className="sr-only">Search tasks by title or identifier.</DialogDescription>
        {/* the list is filtered here (server search + client identifier match); cmdk owns highlight and keys */}
        <Command shouldFilter={false} className="min-h-0 bg-transparent p-0">
          <CommandInput autoFocus placeholder="Search tasks…" value={query} onValueChange={setQuery} />
          <CommandList className="mx-1.5 mt-1 mb-1.5 max-h-none min-h-0 flex-1 rounded-lg bg-background p-1 ring-1 ring-border">
            <CommandEmpty className="p-6 text-[13px] text-muted-foreground">{loading ? 'Searching…' : 'No matching tasks'}</CommandEmpty>
            <CommandGroup className="p-0">
              {candidates.map((task) => (
                <CommandItem
                  key={task.id}
                  value={task.id}
                  className="h-9 gap-2.5 px-2.5 text-[13px]"
                  onSelect={() => {
                    onSelect(task)
                    onOpenChange(false)
                  }}
                >
                  <TaskStatusIcon status={statuses.find((status) => status.id === task.statusId)} />
                  <span className="w-[76px] shrink-0 text-xs text-muted-foreground/70 tabular-nums">{task.identifier}</span>
                  <span className="min-w-0 flex-1 truncate">{task.title || 'Untitled'}</span>
                  <span className="max-w-[140px] shrink-0 truncate text-xs text-muted-foreground/70">{projectName(task.projectId)}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  )
}
