import { Hierarchy, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import type { TaskRelationRecord } from '@/api/generated/types.gen'
import { taskIdentifier, type Project, type TaskRef, type TaskStatusDef } from '@/features/tasks/api/models'
import { ADD_RELATION_OPTIONS, groupRelations, type RelationKind } from '@/features/tasks/relationsLib'
import { TaskStatusIcon } from './TaskStatusIcon'

const MENU = 'flex w-auto min-w-[200px] flex-col gap-px p-1'
const OPTION = 'group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground'
/** Press feedback (emil-design-eng): 0.97 on :active, 160ms ease-out; replaces the Button's 1px press nudge. */
const PRESS = 'transition-transform duration-160 ease-out active:not-aria-[haspopup]:translate-y-0 active:scale-[0.97]'
const DUPLICATE_FALLBACK = { category: 'duplicate', color: '#8b8f98' } as const

const projectOf = (projects: Project[], projectId: string) => projects.find((project) => project.id === projectId)

/** Flat strip above the title while the task is a duplicate: glyph, canonical task (opens it), Unmark. */
export function DuplicateBanner({ duplicateOf, projects, status, animate, pending, onOpen, onUnmark }: {
  duplicateOf: TaskRef
  projects: Project[]
  status: TaskStatusDef | undefined
  animate: boolean
  pending: boolean
  onOpen: (taskId: string) => void
  onUnmark: () => void
}) {
  const identifier = taskIdentifier(duplicateOf.id, projectOf(projects, duplicateOf.projectId))
  return (
    <div
      role="note"
      aria-label={`Duplicate of ${identifier}`}
      className={cn('mb-5 flex min-h-10 min-w-0 items-center gap-2 border-b border-border pb-2 text-[13px] text-muted-foreground', animate && 'animate-relation-enter')}
    >
      <TaskStatusIcon status={status ?? DUPLICATE_FALLBACK} />
      <span className="shrink-0">Duplicate of</span>
      <button
        type="button"
        className="flex min-w-0 items-center gap-1.5 rounded-sm text-foreground underline-offset-2 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
        onClick={() => onOpen(duplicateOf.id)}
      >
        <span className="shrink-0 text-muted-foreground tabular-nums">{identifier}</span>
        <span className="truncate font-medium">{duplicateOf.title || 'Untitled'}</span>
      </button>
      <div className="flex-1" />
      <Button variant="ghost" size="xs" className={cn('shrink-0 text-muted-foreground', PRESS)} disabled={pending} onClick={onUnmark}>
        Unmark
      </Button>
    </div>
  )
}

/**
 * Main-column section like "GitHub": one aligned list. The kind ("Blocks", "Related"…) sits in a quiet left column,
 * named once beside the first row of its group; the hover fill bleeds past the text edge so rows line up with the heading.
 */
export function TaskRelationsSection({ relations, statuses, projects, newRelationId, removingId, onOpen, onRemove }: {
  relations: TaskRelationRecord[]
  statuses: TaskStatusDef[]
  projects: Project[]
  newRelationId: string | null
  removingId: string | undefined
  onOpen: (taskId: string) => void
  onRemove: (relationId: string) => void
}) {
  const groups = groupRelations(relations)
  if (groups.length === 0) return null
  const count = groups.reduce((total, group) => total + group.relations.length, 0)
  return (
    <section aria-label="Relations" className="mt-6 border-t border-border pt-4">
      <h3 className="mb-2 flex items-baseline gap-1.5 text-xs font-semibold text-muted-foreground">
        Relations
        <span className="font-normal text-muted-foreground/60 tabular-nums">{count}</span>
      </h3>
      {/* one grid for every row (kind · status · id · title · remove): subgrid rows share its columns, so every title
          starts at the same x without fixed widths; -mx-2 lets the hover fill bleed past the text edge */}
      <div className="-mx-2 grid grid-cols-[max-content_max-content_max-content_minmax(0,1fr)_auto] gap-y-px">
        {groups.map((group) => (
          <ul key={group.key} role="group" aria-label={group.label} className="col-span-full m-0 grid list-none grid-cols-subgrid gap-y-px p-0">
            {group.relations.map((relation, index) => {
              const identifier = taskIdentifier(relation.task.id, projectOf(projects, relation.task.project_id))
              return (
                <li
                  key={relation.id}
                  className={cn('group/relation col-span-full grid min-h-8 grid-cols-subgrid items-center rounded-md hover:bg-muted', relation.id === newRelationId && 'animate-relation-enter')}
                >
                  <button
                    type="button"
                    className="col-span-4 grid grid-cols-subgrid items-center rounded-md py-1.5 pl-2 text-left text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                    onClick={() => onOpen(relation.task.id)}
                  >
                    <span aria-hidden="true" className="pr-5 whitespace-nowrap text-muted-foreground/70 max-sm:pr-3">{index === 0 ? group.label : null}</span>
                    <TaskStatusIcon status={statuses.find((status) => status.id === relation.task.status_id)} />
                    <span className="pr-2.5 pl-2 whitespace-nowrap text-muted-foreground tabular-nums">{identifier}</span>
                    <span className="truncate pr-2 text-foreground">{relation.task.title || 'Untitled'}</span>
                  </button>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label={`Remove relation to ${identifier}`}
                    disabled={removingId === relation.id}
                    className={cn('mr-1 shrink-0 text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/relation:opacity-100 focus-visible:opacity-100', PRESS)}
                    onClick={() => onRemove(relation.id)}
                  >
                    <X className="size-3" />
                  </Button>
                </li>
              )
            })}
          </ul>
        ))}
      </div>
    </section>
  )
}

/** Ghost button beside Attach: Blocks…, Blocked by…, Related to…, Mark as duplicate of… (each opens the picker). */
export function AddRelationMenu({ onChoose }: { onChoose: (kind: RelationKind) => void }) {
  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger
        render={
          <Button variant="ghost" className="text-xs text-muted-foreground/70">
            <Hierarchy className="size-3.5" aria-hidden="true" />
            Add relation
          </Button>
        }
      />
      <DropdownMenuContent className={MENU}>
        {ADD_RELATION_OPTIONS.map((option) => (
          <DropdownMenuItem key={option.kind} className={OPTION} onClick={() => onChoose(option.kind)}>
            {option.label}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
