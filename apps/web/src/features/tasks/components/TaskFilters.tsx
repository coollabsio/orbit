import { ArrowUpDown, Columns3, Filter, List, Search, SlidersHorizontal } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { UserAvatar } from '@/components/common/UserAvatar'
import { TaskStatusIcon } from './TaskStatusIcon'
import type { User } from '@/features/workspaces/models'
import { SORT_OPTIONS, type SortKey, type StatusGroup } from '@/features/tasks/tasksLib'

const MENU = 'flex w-auto min-w-[180px] flex-col gap-px p-1'
const OPTION =
  `group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-[selected]:bg-accent data-[selected]:font-medium`
const HEADING = 'px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase'
const SEP = 'my-1 shrink-0'
const FILTER_BTN =
  'data-[active]:bg-primary/10 data-[active]:text-primary data-[active]:ring-1 data-[active]:ring-inset data-[active]:ring-primary/25 max-[899px]:w-8 max-[899px]:px-0'
/** Shared control tokens on the wrapper; the single focus ring lives there too, so the inner input adds none. */
const SEARCH_GROUP =
  'h-8 w-auto min-w-[180px] rounded-lg border border-input bg-muted transition-colors has-[[data-slot=input-group-control]:focus-visible]:border-ring has-[[data-slot=input-group-control]:focus-visible]:ring-1 has-[[data-slot=input-group-control]:focus-visible]:ring-ring dark:bg-muted max-[899px]:order-10 max-[899px]:mt-1 max-[899px]:h-[34px] max-[899px]:min-w-0 max-[899px]:basis-full'

interface TaskFiltersProps {
  users: User[]
  groups: StatusGroup[]
  statusKey: string | null
  assigneeId: string | null
  sort: SortKey
  layout: 'list' | 'board'
  search: string
  onStatusChange: (key: string | null) => void
  onAssigneeChange: (assigneeId: string | null) => void
  onSortChange: (sort: SortKey) => void
  onLayoutChange: (layout: 'list' | 'board') => void
  onSearchChange: (search: string) => void
}

/** Header dropdowns: "Filter" (status + assignee), "Sort" (order inside groups) and "Display" (layout). */
export function TaskFilters({
  users,
  groups,
  statusKey,
  assigneeId,
  sort,
  layout,
  onStatusChange,
  onAssigneeChange,
  onSortChange,
  onLayoutChange,
  search,
  onSearchChange,
}: TaskFiltersProps) {
  const activeCount = (statusKey ? 1 : 0) + (assigneeId ? 1 : 0)
  const sortLabel = SORT_OPTIONS.find((o) => o.key === sort)?.label ?? 'Sort'

  return (
    <>
      <InputGroup className={SEARCH_GROUP}>
        <InputGroupAddon align="inline-start" className="pl-[9px]">
          <Search className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        </InputGroupAddon>
        <InputGroupInput type="search" aria-label="Search tasks" placeholder="Search tasks" value={search} onChange={(event) => onSearchChange(event.target.value)} className="h-auto border-0 text-sm text-foreground shadow-none outline-none placeholder:text-muted-foreground focus:outline-none focus-visible:ring-0 md:text-sm" />
      </InputGroup>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button variant="ghost" className={FILTER_BTN} data-active={activeCount > 0 || undefined} aria-label="Filter tasks">
              <Filter className="size-4" />
              <span className="max-[899px]:hidden">Filter</span>
              {activeCount > 0 ? <span className="inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-[10px] font-semibold text-primary-foreground">{activeCount}</span> : null}
            </Button>
          }
        />
        <DropdownMenuContent align="end" className={MENU}>
          <DropdownMenuGroup className="flex flex-col gap-px">
            <DropdownMenuLabel className={HEADING}>Status</DropdownMenuLabel>
            {groups.map((group) => (
              <DropdownMenuItem
                key={group.key}
                className={OPTION}
                data-selected={group.key === statusKey || undefined}
                onClick={() => onStatusChange(group.key === statusKey ? null : group.key)}
              >
                <TaskStatusIcon status={group.status} />
                {group.name}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
          <DropdownMenuSeparator className={SEP} />
          <DropdownMenuGroup className="flex flex-col gap-px">
            <DropdownMenuLabel className={HEADING}>Assignee</DropdownMenuLabel>
            {users.map((u) => (
              <DropdownMenuItem
                key={u.id}
                className={OPTION}
                data-selected={u.id === assigneeId || undefined}
                onClick={() => onAssigneeChange(u.id === assigneeId ? null : u.id)}
              >
                <UserAvatar user={u} size={16} />
                {u.name}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
          {activeCount > 0 ? (
            <>
              <DropdownMenuSeparator className={SEP} />
              <DropdownMenuItem
                className={OPTION}
                onClick={() => {
                  onStatusChange(null)
                  onAssigneeChange(null)
                }}
              >
                Clear filters
              </DropdownMenuItem>
            </>
          ) : null}
        </DropdownMenuContent>
      </DropdownMenu>

      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button variant="ghost" className={FILTER_BTN} data-active={sort !== 'manual' || undefined} aria-label={`Sort tasks: ${sortLabel}`}>
              <ArrowUpDown className="size-4" />
              <span className="max-[899px]:hidden">{sort === 'manual' ? 'Sort' : sortLabel}</span>
            </Button>
          }
        />
        <DropdownMenuContent align="end" className={MENU}>
          <DropdownMenuGroup className="flex flex-col gap-px">
            <DropdownMenuLabel className={HEADING}>Sort by</DropdownMenuLabel>
            {SORT_OPTIONS.map((option) => (
              <DropdownMenuItem
                key={option.key}
                className={OPTION}
                data-selected={option.key === sort || undefined}
                onClick={() => onSortChange(option.key)}
              >
                {option.label}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>

      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button variant="ghost" className={FILTER_BTN} aria-label="Display options">
              <SlidersHorizontal className="size-4" />
              <span className="max-[899px]:hidden">Display</span>
            </Button>
          }
        />
        <DropdownMenuContent align="end" className={MENU}>
          <DropdownMenuGroup className="flex flex-col gap-px">
            <DropdownMenuLabel className={HEADING}>Layout</DropdownMenuLabel>
            <DropdownMenuItem className={OPTION} data-selected={layout === 'list' || undefined} onClick={() => onLayoutChange('list')}>
              <List className="size-3.5" />
              List
            </DropdownMenuItem>
            <DropdownMenuItem className={OPTION} data-selected={layout === 'board' || undefined} onClick={() => onLayoutChange('board')}>
              <Columns3 className="size-3.5" />
              Board
            </DropdownMenuItem>
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  )
}
