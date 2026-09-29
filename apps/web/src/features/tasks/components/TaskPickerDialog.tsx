import { useState } from 'react'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { useProjects } from '@/features/tasks/api/projects'
import { useTaskCandidates } from '@/features/tasks/useTaskCandidates'
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

/**
 * Pick another task (duplicate target, blocker, related). Same data pattern as the command palette; the title is
 * rendered inside the popup so it names the dialog. Mount only while open: the queries start on mount. Closing
 * plays the exit animation first and reports `onOpenChange(false)` once it has finished, so the caller unmounts it then.
 */
export function TaskPickerDialog({ open, onOpenChange, title, statuses, excludeIds = [], excludeDuplicates = false, onSelect }: TaskPickerDialogProps) {
  const { workspace } = useWorkspace()
  const projects = useProjects(workspace.id)
  const [query, setQuery] = useState('')
  const [closing, setClosing] = useState(false)
  const { candidates, loading } = useTaskCandidates({ query, excludeIds, excludeDuplicates, statuses })
  const projectName = (projectId: string) => projects.data?.find((project) => project.id === projectId)?.name

  return (
    <Dialog
      open={open && !closing}
      onOpenChange={(next) => { if (!next) setClosing(true) }}
      onOpenChangeComplete={(next) => { if (!next) onOpenChange(false) }}
    >
      <DialogContent
        showCloseButton={false}
        className="top-[12vh] max-h-[min(60vh,28rem)] translate-y-0 gap-0 overflow-hidden p-0 sm:max-w-[576px]"
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
                    // the popup stays clickable through its exit animation: a second click must not pick again
                    if (closing) return
                    onSelect(task)
                    setClosing(true)
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
