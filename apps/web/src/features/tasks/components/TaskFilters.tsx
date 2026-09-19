import { ArrowUpDown, Columns3, Filter, List, Search, SlidersHorizontal } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { UserAvatar } from '../../../components/ui/UserAvatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import type { User } from '../api/models'
import { SORT_OPTIONS, type SortKey, type StatusGroup } from '../tasksLib'

const MENU = 'flex min-w-[180px] flex-col gap-px p-1'
const OPTION =
  'group flex w-full min-h-8 items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-accent data-[selected]:bg-accent data-[selected]:font-medium'
const HEADING = 'px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase'
const SEP = 'my-1 h-px shrink-0 bg-border'
const FILTER_BTN =
  'data-[active]:bg-primary/10 data-[active]:text-primary data-[active]:ring-1 data-[active]:ring-inset data-[active]:ring-primary/25 max-[899px]:w-8 max-[899px]:px-0'

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
      <label className="flex h-8 min-w-[180px] items-center gap-1.5 rounded-lg border border-input bg-muted px-[9px] text-foreground transition-colors focus-within:border-ring focus-within:ring-1 focus-within:ring-ring max-[899px]:order-10 max-[899px]:mt-1 max-[899px]:h-[34px] max-[899px]:min-w-0 max-[899px]:basis-full">
        <Search className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        <input type="search" aria-label="Search tasks" placeholder="Search tasks" value={search} onChange={(event) => onSearchChange(event.target.value)} className="w-full border-0 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground focus:outline-none" />
      </label>
      <Dropdown
        align="right"
        trigger={() => (
          <Button variant="ghost" className={FILTER_BTN} data-active={activeCount > 0 || undefined} aria-label="Filter tasks">
            <Filter className="size-4" />
            <span className="max-[899px]:hidden">Filter</span>
            {activeCount > 0 ? <span className="inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-[10px] font-semibold text-primary-foreground">{activeCount}</span> : null}
          </Button>
        )}
      >
        {(close) => (
          <div className={MENU}>
            <div className={HEADING}>Status</div>
            {groups.map((group) => (
              <button
                key={group.key}
                className={OPTION}
                data-selected={group.key === statusKey || undefined}
                onClick={() => {
                  onStatusChange(group.key === statusKey ? null : group.key)
                  close()
                }}
              >
                <TaskStatusIcon status={group.status} />
                {group.name}
              </button>
            ))}
            <div className={SEP} />
            <div className={HEADING}>Assignee</div>
            {users.map((u) => (
              <button
                key={u.id}
                className={OPTION}
                data-selected={u.id === assigneeId || undefined}
                onClick={() => {
                  onAssigneeChange(u.id === assigneeId ? null : u.id)
                  close()
                }}
              >
                <UserAvatar user={u} size={16} />
                {u.name}
              </button>
            ))}
            {activeCount > 0 ? (
              <>
                <div className={SEP} />
                <button
                  className={OPTION}
                  onClick={() => {
                    onStatusChange(null)
                    onAssigneeChange(null)
                    close()
                  }}
                >
                  Clear filters
                </button>
              </>
            ) : null}
          </div>
        )}
      </Dropdown>

      <Dropdown
        align="right"
        trigger={() => (
          <Button variant="ghost" className={FILTER_BTN} data-active={sort !== 'manual' || undefined} aria-label={`Sort tasks: ${sortLabel}`}>
            <ArrowUpDown className="size-4" />
            <span className="max-[899px]:hidden">{sort === 'manual' ? 'Sort' : sortLabel}</span>
          </Button>
        )}
      >
        {(close) => (
          <div className={MENU}>
            <div className={HEADING}>Sort by</div>
            {SORT_OPTIONS.map((option) => (
              <button
                key={option.key}
                className={OPTION}
                data-selected={option.key === sort || undefined}
                onClick={() => {
                  onSortChange(option.key)
                  close()
                }}
              >
                {option.label}
              </button>
            ))}
          </div>
        )}
      </Dropdown>

      <Dropdown
        align="right"
        trigger={() => (
          <Button variant="ghost" className={FILTER_BTN} aria-label="Display options">
            <SlidersHorizontal className="size-4" />
            <span className="max-[899px]:hidden">Display</span>
          </Button>
        )}
      >
        {(close) => (
          <div className={MENU}>
            <div className={HEADING}>Layout</div>
            <button
              className={OPTION}
              data-selected={layout === 'list' || undefined}
              onClick={() => {
                onLayoutChange('list')
                close()
              }}
            >
              <List className="size-3.5" />
              List
            </button>
            <button
              className={OPTION}
              data-selected={layout === 'board' || undefined}
              onClick={() => {
                onLayoutChange('board')
                close()
              }}
            >
              <Columns3 className="size-3.5" />
              Board
            </button>
          </div>
        )}
      </Dropdown>
    </>
  )
}
