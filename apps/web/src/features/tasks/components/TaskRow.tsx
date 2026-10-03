import { taskRowTarget } from '@/shortcuts/taskTarget'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { BlockedIndicator } from './BlockedIndicator'
import { refIdentifier, type Project, type Task, type TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import type { LabelRecord } from '@/api/generated/types.gen'
import type { TaskProperty } from '@/features/views/viewState'
import { PriorityPicker } from './PriorityPicker'
import { StatusPicker } from './StatusPicker'
import { AssigneePicker } from './AssigneePicker'
import { LinkifiedText } from './LinkifiedText'
import { DateStamp, ProjectChip } from './TaskPropertyChips'
import { DueDatePicker } from './DueDatePicker'
import { SubIssueProgress, completedStatusColor } from './SubIssueProgress'
import { TreeGutter } from './TreeGutter'
import { NestChip } from './NestChip'
import { LabelPill } from './TaskLabels'
import type { NestRowProps } from '@/features/views/useNestDrop'

/** A row's place in the nested list's tree. */
export interface TaskRowTree {
  depth: number
  hasChildren: boolean
  expanded: boolean
  onToggle: () => void
}

interface TaskRowProps {
  task: Task
  statuses: TaskStatusDef[]
  labels: LabelRecord[]
  users: User[]
  assignees: User[]
  project: Project | undefined
  /** Only these properties render (`display.properties`). */
  properties: TaskProperty[]
  selected: boolean
  dragging: boolean
  /** False when no drop could change anything (e.g. grouped by project with a non-manual order). */
  draggable: boolean
  /** Where the insertion line shows during a manual-order drag. */
  dropEdge: 'top' | 'bottom' | null
  onOpen: (taskId: string) => void
  onToggleSelect: (taskId: string) => void
  onDragStart: (taskId: string) => void
  onDragEnd: () => void
  /** Duplicate needs a canonical task: the list opens its picker. */
  onRequestDuplicate: (task: Task) => void
  /** Nested list only: indentation, guides and the chevron slot. Null in flat lists. */
  tree: TaskRowTree | null
  /** Muted "Parent title ›" before the title: flat lists, and nested roots whose parent is not in the result. */
  showParent: boolean
  /** Drop onto the row (`useNestDrop`): nest into it, or become a sibling of a sub-issue. */
  nest?: NestRowProps
}

/** List row: [checkbox] priority · id · [tree gutter] status · [parent ›] title … labels · progress · project · due · assignee · created · updated. */
export function TaskRow({ task, statuses, labels, users, assignees, project, properties, selected, dragging, draggable, dropEdge, onOpen, onToggleSelect, onDragStart, onDragEnd, onRequestDuplicate, tree, showParent, nest }: TaskRowProps) {
  const status = statuses.find((s) => s.id === task.statusId)
  const has = (property: TaskProperty) => properties.includes(property)
  return (
    <div
      className={cn(
        'group/row relative flex min-h-10 w-full min-w-0 cursor-pointer items-center gap-2 border-b px-3 py-1.5 text-left transition-colors outline-none hover:bg-foreground/[0.02] focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset data-dragging:bg-muted data-dragging:opacity-50 data-selected:bg-primary/10 max-[480px]:gap-1.5',
        // 2px insertion line on the row edge while a manual-order drag hovers it; absolute, so nothing shifts
        'data-drop-edge:before:pointer-events-none data-drop-edge:before:absolute data-drop-edge:before:inset-x-0 data-drop-edge:before:z-[1] data-drop-edge:before:h-0.5 data-drop-edge:before:bg-primary data-[drop-edge=top]:before:-top-px data-[drop-edge=bottom]:before:-bottom-px',
        // tint + inset ring while a drop would nest into the row (the chip names the action)
        'data-[nest=inside]:bg-primary/10 data-[nest=inside]:ring-1 data-[nest=inside]:ring-primary/40 data-[nest=inside]:ring-inset',
      )}
      data-task-row
      {...taskRowTarget(task.id)}
      data-depth={tree?.depth ?? 0}
      data-selected={selected || undefined}
      data-dragging={dragging || undefined}
      data-drop-edge={dropEdge ?? undefined}
      role="button"
      tabIndex={0}
      draggable={draggable}
      onDragStart={draggable ? (e) => {
        e.dataTransfer.effectAllowed = 'move'
        e.dataTransfer.setData('text/task-id', task.id)
        onDragStart(task.id)
      } : undefined}
      onDragEnd={onDragEnd}
      onClick={() => onOpen(task.id)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && e.target === e.currentTarget) onOpen(task.id)
      }}
      {...nest}
    >
      <div className="relative -my-1.5 -ml-3 flex w-7 shrink-0 cursor-pointer self-stretch" onClick={(e) => e.stopPropagation()}>
        {/* the ::after overlay stretches the hit area over the whole 28px strip (the old label click target) */}
        <Checkbox
          checked={selected}
          aria-label={`Select ${task.identifier}`}
          onCheckedChange={() => onToggleSelect(task.id)}
          className="absolute top-1/2 left-3 -translate-y-1/2 cursor-pointer opacity-0 transition-opacity group-hover/row:opacity-100 group-data-selected/row:opacity-100 after:-top-3 after:-bottom-3 after:-left-3 after:right-0 focus-visible:opacity-100"
        />
      </div>
      {has('priority') ? <PriorityPicker task={task} /> : null}
      {/* the blocked mark lives inside the fixed id column so titles stay aligned; on phones only the mark shows */}
      {has('id') ? (
        <span className={cn('inline-flex w-[72px] shrink-0 items-center gap-1 text-xs whitespace-nowrap text-muted-foreground/70 tabular-nums max-[480px]:w-auto', !task.blocked && 'max-[480px]:hidden')}>
          <span className="whitespace-nowrap max-[480px]:hidden">{task.identifier}</span>
          {task.blocked ? <BlockedIndicator /> : null}
        </span>
      ) : null}
      {/* only the status + title area indents; the checkbox, priority and id columns stay aligned */}
      {tree ? (
        <TreeGutter
          depth={tree.depth}
          hasChildren={tree.hasChildren}
          expanded={tree.expanded}
          onToggle={tree.onToggle}
          identifier={task.identifier}
          className="-my-1.5"
        />
      ) : null}
      {has('status') ? <StatusPicker task={task} statuses={statuses} onRequestDuplicate={() => onRequestDuplicate(task)} /> : null}
      {!has('id') && task.blocked ? <BlockedIndicator /> : null}
      {showParent && task.parent ? (
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[13px]">
          <Button
            type="button"
            variant="link"
            title={`Open ${refIdentifier(task.parent)}`}
            className="h-auto max-w-[40%] shrink justify-start p-0 text-[13px] font-normal text-muted-foreground hover:text-foreground"
            onClick={(event) => {
              event.stopPropagation()
              onOpen(task.parent!.id)
            }}
          >
            <span className="truncate">{task.parent.title || 'Untitled'}</span>
          </Button>
          <span aria-hidden className="shrink-0 text-muted-foreground/50">›</span>
          <span className="min-w-0 flex-1 truncate font-medium"><LinkifiedText text={task.title || 'Untitled'} /></span>
        </span>
      ) : (
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium"><LinkifiedText text={task.title || 'Untitled'} /></span>
      )}
      {has('labels') && task.labels.length > 0 ? (
        <span className="flex shrink-0 gap-1 max-[1099px]:hidden">
          {task.labels.map((labelId) => {
            const label = labels.find((item) => item.id === labelId)
            return label ? <LabelPill key={label.id} label={label} /> : null
          })}
        </span>
      ) : null}
      {/* fixed-width progress, project, due date and assignee slots keep the columns aligned across rows;
          the progress slot is a min width (right-aligned) so rare 3-digit counts grow it instead of overflowing */}
      {has('sub_issue_progress') ? (
        <span className="flex min-w-[3.5rem] shrink-0 justify-end max-[640px]:hidden">
          {(task.subIssueCount ?? 0) > 0 ? <SubIssueProgress closed={task.subIssueClosedCount ?? 0} total={task.subIssueCount ?? 0} color={completedStatusColor(statuses, task.projectId)} /> : null}
        </span>
      ) : null}
      {has('project') ? <span className="flex w-[4.5rem] shrink-0 max-[1099px]:hidden"><ProjectChip project={project} className="max-w-full text-xs" /></span> : null}
      {has('due_date') ? <DueDatePicker task={task} status={status} className="w-[4.25rem] shrink-0 text-xs max-[640px]:hidden" /> : null}
      {/* at most two circles (28px) so the slot stays narrow */}
      {has('assignee') ? <AssigneePicker task={task} users={users} max={assignees.length > 2 ? 1 : 2} className="w-7 justify-end" /> : null}
      {has('created') ? <DateStamp property="created" iso={task.createdAt} className="min-w-[44px] text-right text-xs text-muted-foreground/70 max-[480px]:hidden" /> : null}
      {has('updated') ? <DateStamp property="updated" iso={task.updatedAt} className="min-w-[44px] text-right text-xs text-muted-foreground/70 max-[480px]:hidden" /> : null}
      {nest?.['data-nest'] === 'inside' ? <NestChip className="top-1/2 right-3 -translate-y-1/2" /> : null}
    </div>
  )
}
