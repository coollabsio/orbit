import { useRef, useState, type Ref } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Add as Plus, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import type { PageTaskRecord } from '@/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { UserAvatarStack } from '@/components/common/UserAvatar'
import { taskFromRecord, type Project, type Task, type TaskStatusDef } from '@/features/tasks/api/models'
import { subIssuesQuery, useCreateTask, useSubIssues, useUpdateTask } from '@/features/tasks/api/tasks'
import { subIssueDefaults } from '@/features/tasks/subIssuesLib'
import { defaultStatusOf, projectStatuses } from '@/features/tasks/taskMeta'
import { useParentActions } from '@/features/tasks/useParentActions'
import { useCollapsedTasks } from '@/features/views/useCollapsedTasks'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { SubIssueProgress, completedStatusColor } from './SubIssueProgress'
import { TaskStatusIcon } from './TaskStatusIcon'
import { TreeGutter } from './TreeGutter'

/** Rows bleed 8px past the text edge (like Relations) so the hover fill frames them; the gutter starts after it. */
const ROW_INSET = 8
/** Closed for counts: completed, cancelled and duplicate (the server's rule for `sub_issue_closed_count`). */
const CLOSED = new Set(['completed', 'cancelled', 'duplicate'])

export interface SubIssuesSectionProps {
  parent: Task
  projects: Project[]
  statuses: TaskStatusDef[]
  users: User[]
  currentUserId: string
  /** The inline composer is open (the header's "Add sub-issue" opens it too). */
  composing: boolean
  onComposingChange: (open: boolean) => void
  onAddExisting: () => void
  onOpen: (taskId: string) => void
  /** The header "+", so the page can return focus to it when Esc closes the composer. */
  addButtonRef?: Ref<HTMLButtonElement>
}

type Context = Omit<SubIssuesSectionProps, 'parent' | 'composing' | 'onComposingChange' | 'onAddExisting' | 'addButtonRef'> & {
  collapsed: ReadonlySet<string>
  toggle: (id: string) => void
  newIds: ReadonlySet<string>
}

/** Direct sub-issues of `parentId`, oldest first, as view models. */
function useChildren(parentId: string, projects: Project[]): Task[] {
  const { workspace } = useWorkspace()
  const records = useSubIssues(workspace.id, parentId).data?.items ?? []
  return records.map((record) => taskFromRecord(record, projects.find((project) => project.id === record.project_id)))
}

/** Main-column section above Relations: progress, the sub-issue tree (same gutter as the list) and the inline composer. */
export function SubIssuesSection({ parent, composing, onComposingChange, onAddExisting, addButtonRef, ...rest }: SubIssuesSectionProps) {
  const { workspace } = useWorkspace()
  const composerInputRef = useRef<HTMLInputElement>(null)
  const children = useChildren(parent.id, rest.projects)
  const { collapsed, toggle } = useCollapsedTasks(workspace.id)
  const [newIds, setNewIds] = useState<string[]>([])
  if (children.length === 0 && !composing) return null
  const statusOf = (task: Task) => rest.statuses.find((status) => status.id === task.statusId)
  const closed = children.filter((task) => CLOSED.has(statusOf(task)?.category ?? '')).length
  const context: Context = { ...rest, collapsed, toggle, newIds: new Set(newIds) }
  return (
    <section aria-label="Sub-issues" className="mt-6 border-t pt-4">
      <div className="mb-2 flex min-h-6 items-center gap-2">
        <h3 className="text-xs font-semibold text-muted-foreground">Sub-issues</h3>
        {children.length > 0 ? <SubIssueProgress closed={closed} total={children.length} color={completedStatusColor(rest.statuses, parent.projectId)} /> : null}
        <div className="flex-1" />
        <Button variant="ghost" size="xs" className="text-muted-foreground" onClick={onAddExisting}>Add existing</Button>
        <Button
          ref={addButtonRef}
          variant="ghost"
          size="icon-xs"
          className="text-muted-foreground"
          aria-label="Add sub-issue"
          title="Add sub-issue"
          aria-pressed={composing}
          // already composing: take the user back to the field instead of doing nothing
          onClick={() => (composing ? composerInputRef.current?.focus() : onComposingChange(true))}
        >
          <Plus className="size-3.5" />
        </Button>
      </div>
      <div className="-mx-2">
        {children.length > 0 ? (
          <ul aria-label="Sub-issues" className="m-0 flex list-none flex-col gap-px p-0">
            {children.map((task) => <SubIssueNode key={task.id} task={task} depth={0} context={context} />)}
          </ul>
        ) : null}
        {composing ? (
          <SubIssueComposer
            inputRef={composerInputRef}
            parent={parent}
            siblings={children}
            statuses={rest.statuses}
            currentUserId={rest.currentUserId}
            onCreated={(id) => setNewIds((ids) => [...ids, id])}
            onClose={() => onComposingChange(false)}
          />
        ) : null}
      </div>
    </section>
  )
}

function SubIssueNode({ task, depth, context }: { task: Task; depth: number; context: Context }) {
  const expanded = !context.collapsed.has(task.id)
  return (
    <li>
      <SubIssueRow task={task} depth={depth} context={context} />
      {/* each level loads its own children when shown; no height animation, the rows fade in with a 4px drop */}
      {(task.subIssueCount ?? 0) > 0 && expanded ? <SubIssueChildren parentId={task.id} depth={depth + 1} context={context} /> : null}
    </li>
  )
}

function SubIssueChildren({ parentId, depth, context }: { parentId: string; depth: number; context: Context }) {
  const children = useChildren(parentId, context.projects)
  if (children.length === 0) return null
  return (
    <ul aria-label="Sub-issues" className="m-0 mt-px flex animate-relation-enter list-none flex-col gap-px p-0 motion-reduce:animate-none">
      {children.map((task) => <SubIssueNode key={task.id} task={task} depth={depth} context={context} />)}
    </ul>
  )
}

function SubIssueRow({ task, depth, context }: { task: Task; depth: number; context: Context }) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const parentActions = useParentActions(workspace.id)
  const status = context.statuses.find((item) => item.id === task.statusId)
  // Duplicate needs a canonical task: change it from the task itself
  const options = projectStatuses(context.statuses, task.projectId).filter((option) => option.category !== 'duplicate')
  const assignees = context.users.filter((user) => task.assigneeIds.includes(user.id))
  return (
    <div
      className={cn('group/sub relative flex min-h-8 items-center gap-1.5 rounded-md pr-1 text-xs transition-colors duration-150 hover-fine:hover:bg-muted motion-reduce:transition-none', context.newIds.has(task.id) && 'animate-relation-enter')}
      data-depth={depth}
    >
      <TreeGutter
        depth={depth}
        inset={ROW_INSET}
        hasChildren={(task.subIssueCount ?? 0) > 0}
        expanded={!context.collapsed.has(task.id)}
        onToggle={() => context.toggle(task.id)}
        identifier={task.identifier}
      />
      <DropdownMenu modal={false}>
        <DropdownMenuTrigger render={
          <Button variant="ghost" size="icon-sm" className="size-[22px] shrink-0" aria-label={`Status: ${status?.name ?? 'None'}`}>
            <TaskStatusIcon status={status} />
          </Button>
        } />
        <DropdownMenuContent className="w-auto min-w-45">
          {options.map((option) => (
            <DropdownMenuItem
              key={option.id}
              className="data-selected:bg-accent data-selected:font-medium"
              data-selected={option.id === task.statusId || undefined}
              onClick={() => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, status_id: option.id } })}
            >
              <TaskStatusIcon status={option} />
              {option.name}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      <Button
        type="button"
        variant="link"
        className="h-auto min-w-0 flex-1 shrink justify-start gap-2 self-stretch p-0 text-xs font-normal text-foreground"
        onClick={() => context.onOpen(task.id)}
      >
        <span className="shrink-0 text-muted-foreground tabular-nums">{task.identifier}</span>
        <span className="truncate">{task.title || 'Untitled'}</span>
      </Button>
      {assignees.length > 0 ? <UserAvatarStack users={assignees} size={16} /> : null}
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={`Remove ${task.identifier} from parent`}
        title="Remove from parent"
        className="shrink-0 text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/sub:opacity-100 focus-visible:opacity-100"
        onClick={() => void parentActions.setParent([task], null)}
      >
        <X className="size-3" />
      </Button>
    </div>
  )
}

/**
 * Inline "new sub-issue" row: Enter creates one with the parent's defaults and keeps the field focused for the next;
 * Esc closes the composer only (the task page closes on a document-level Esc).
 */
function SubIssueComposer({ inputRef, parent, siblings, statuses, currentUserId, onCreated, onClose }: {
  inputRef: Ref<HTMLInputElement>
  parent: Task
  siblings: Task[]
  statuses: TaskStatusDef[]
  currentUserId: string
  onCreated: (taskId: string) => void
  onClose: () => void
}) {
  const { workspace } = useWorkspace()
  const queryClient = useQueryClient()
  const createTask = useCreateTask(workspace.id)
  const [title, setTitle] = useState('')
  // the field's live value, read after an await (a failed create only refills an empty field)
  const titleRef = useRef('')
  const changeTitle = (value: string) => {
    titleRef.current = value
    setTitle(value)
  }
  // creates run one after another, so rapid entries keep the order they were typed in
  const queue = useRef<Promise<void>>(Promise.resolve())
  const create = async (text: string) => {
    const defaults = subIssueDefaults(parent, { statuses, currentUserId, siblings })
    if (!defaults) return
    try {
      const record = await createTask.mutateAsync({ ...defaults, title: text })
      // show it at once; the refetch that follows every create confirms the list
      queryClient.setQueryData<PageTaskRecord>(subIssuesQuery(workspace.id, parent.id).queryKey, (page) =>
        page && !page.items.some((item) => item.id === record.id) ? { ...page, items: [...page.items, record] } : page)
      onCreated(record.id)
    } catch {
      // the alert below reports it; the title comes back unless the next one is already being typed
      if (titleRef.current === '') changeTitle(text)
    }
  }
  const submit = () => {
    const text = title.trim()
    if (!text || !subIssueDefaults(parent, { statuses, currentUserId, siblings })) return
    // clear at once: the next title can be typed while this one is created
    changeTitle('')
    queue.current = queue.current.then(() => create(text))
  }
  return (
    <form
      className="mt-px flex min-h-8 items-center gap-1.5 rounded-md bg-muted/50 pr-2 ring-1 ring-transparent focus-within:ring-ring/50"
      onSubmit={(event) => {
        event.preventDefault()
        submit()
      }}
    >
      {/* the gutter's width at depth 0 (ROW_INSET + the 16px chevron slot), so the status icon lines up with the rows above */}
      <span aria-hidden className="w-6 shrink-0" />
      <span className="flex size-[22px] shrink-0 items-center justify-center">
        <TaskStatusIcon status={defaultStatusOf(statuses, parent.projectId)} />
      </span>
      <Input
        ref={inputRef}
        autoFocus
        aria-label="Sub-issue title"
        placeholder="Sub-issue title"
        value={title}
        onChange={(event) => changeTitle(event.target.value)}
        onKeyDown={(event) => {
          // the task page closes on a document-level Esc: this Esc belongs to the composer only
          if (event.key !== 'Escape') return
          event.preventDefault()
          event.stopPropagation()
          onClose()
        }}
        // borderless: the form row carries the fill and the focus ring
        className="h-8 min-w-0 flex-1 rounded-none border-0 bg-transparent px-0 text-[13px] focus-visible:ring-0 md:text-[13px] dark:bg-transparent"
      />
      {createTask.isError ? <span role="alert" className="shrink-0 text-xs text-destructive">Couldn’t create the sub-issue.</span> : null}
    </form>
  )
}
