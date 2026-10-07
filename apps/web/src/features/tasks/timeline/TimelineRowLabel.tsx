import type { ReactNode } from 'react'
import { Gps } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import { DisclosureChevron, GroupIcon } from '@/features/views/components/GroupIcon'
import type { GroupContext } from '@/features/views/grouping'
import type { TaskProperty } from '@/features/views/viewState'
import type { User } from '@/features/workspaces/models'
import { AssigneePicker } from '../components/AssigneePicker'
import { PriorityPicker } from '../components/PriorityPicker'
import { StatusPicker } from '../components/StatusPicker'
import { TaskStatusIcon } from '../components/TaskStatusIcon'
import type { TimelineRow } from './timelineLib'

/** Left-pane cell for a timeline row: group header, "No dates" toggle, or task (its status, priority and assignees
 *  change in place; the rest of the row opens it). */
export function TimelineRowLabel({ row, statuses, users, properties, groupContext, onToggle, onOpen, onReveal }: {
  row: TimelineRow
  statuses: TaskStatusDef[]
  users: User[]
  properties: TaskProperty[]
  groupContext: GroupContext
  onToggle: (key: string, open: boolean) => void
  onOpen: (taskId: string) => void
  /** Scrolls the timeline to this task's bar; only for tasks with dates. */
  onReveal?: () => void
}) {
  if (row.kind === 'group') {
    return (
      <LabelCell fill="group">
        <CellButton className="px-3 font-semibold text-foreground" aria-expanded={row.open} onClick={() => onToggle(row.key, !row.open)}>
          <DisclosureChevron open={row.open} className="size-3.5 text-muted-foreground" />
          <GroupIcon group={row.group} context={groupContext} />
          <span className="min-w-0 flex-1 truncate">{row.group.label}</span>
          <span className="text-xs font-normal text-muted-foreground tabular-nums">{row.done}/{row.total}</span>
        </CellButton>
      </LabelCell>
    )
  }
  if (row.kind === 'undated') {
    return (
      <LabelCell fill="row">
        <CellButton className="pr-3 pl-6 text-xs text-muted-foreground hover:text-foreground" aria-expanded={row.open} onClick={() => onToggle(row.key, !row.open)}>
          <DisclosureChevron open={row.open} />
          No dates ({row.count})
        </CellButton>
      </LabelCell>
    )
  }
  const has = (property: TaskProperty) => properties.includes(property)
  // on phones a dated task is labelled by its bar; only undated tasks need the chip
  return (
    <LabelCell fill="row" className={cn('gap-2 pl-6 max-[899px]:pl-0', row.span && 'max-[899px]:hidden')}>
      {/* on phones the cell is a floating chip: the pickers hide and the chip shows the status glyph */}
      {has('priority') ? <PriorityPicker task={row.task} className="-mx-1 max-[899px]:hidden" /> : null}
      {has('status') ? <StatusPicker task={row.task} statuses={statuses} className="-mx-1 max-[899px]:hidden" /> : null}
      <CellButton className="pr-3 text-foreground" onClick={() => onOpen(row.task.id)}>
        {has('status') ? <span className="hidden max-[899px]:contents"><TaskStatusIcon status={statuses.find((status) => status.id === row.task.statusId)} /></span> : null}
        {has('id') ? <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{row.task.identifier}</span> : null}
        <span className="min-w-0 flex-1 truncate">{row.task.title || 'Untitled'}</span>
      </CellButton>
      {has('assignee') ? (
        <AssigneePicker
          task={row.task}
          users={users}
          max={1}
          className={cn('mr-3 max-[899px]:hidden', row.task.assigneeIds.length === 0 && 'hover-fine:opacity-0 hover-fine:group-hover/row:opacity-100 focus-within:opacity-100 has-data-popup-open:opacity-100')}
        />
      ) : null}
      {onReveal ? (
        // overlays the row end on hover, so it never costs the title any width
        <Tip label="Show on timeline">
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={`Show ${row.task.identifier} on the timeline`}
            // the row-hover fill plus a soft left fade, so the title it covers fades out under it
            className={cn('absolute right-2', has('assignee') && 'right-10', 'bg-[color-mix(in_oklch,var(--foreground)_3%,var(--background))] text-muted-foreground opacity-0 shadow-[-12px_0_8px_-2px_color-mix(in_oklch,var(--foreground)_3%,var(--background))] group-hover/row:opacity-100 focus-visible:opacity-100')}
            onClick={onReveal}
          >
            <Gps aria-hidden="true" />
          </Button>
        </Tip>
      ) : null}
    </LabelCell>
  )
}

/**
 * Sticky left-pane cell; sticky cells need an opaque fill (rows match the row hover band). Below 900px there is no
 * pane: the cell collapses to zero width and its button floats over the track as a small pinned chip.
 */
function LabelCell({ fill, className, children }: { fill: 'group' | 'row'; className?: string; children: ReactNode }) {
  return (
    <div
      data-slot="timeline-label-cell"
      className={cn(
        'sticky left-0 z-10 flex h-8 w-[280px] shrink-0 items-center border-r max-[899px]:w-0 max-[899px]:overflow-visible max-[899px]:border-r-0 max-[899px]:bg-transparent',
        fill === 'group'
          ? 'bg-[color-mix(in_oklch,var(--muted)_45%,var(--background))]'
          : 'bg-background group-hover/row:bg-[color-mix(in_oklch,var(--foreground)_3%,var(--background))]',
        className,
      )}
    >
      {children}
    </div>
  )
}

/** Raw button: it fills the whole cell edge to edge on desktop and turns into the floating chip on phones. */
function CellButton({ className, ...props }: React.ComponentProps<'button'>) {
  return (
    <button
      type="button"
      data-slot="timeline-label-button"
      className={cn(
        'flex h-full min-w-0 flex-1 items-center gap-2 text-left text-[13px] outline-none focus-visible:bg-accent',
        'max-[899px]:ml-2 max-[899px]:h-6 max-[899px]:w-max max-[899px]:flex-none max-[899px]:rounded-md max-[899px]:bg-background/90 max-[899px]:px-2 max-[899px]:shadow-sm',
        className,
      )}
      {...props}
    />
  )
}
