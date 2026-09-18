import { Filter, Hierarchy, Kanban, List, SearchNormal, Setting4, Sort } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import type { User } from '../api/models'
import { SORT_OPTIONS, type SortKey, type StatusGroup } from '../tasksLib'

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
  /** Sub-issues are hidden from the list unless this is on. */
  showSubIssues: boolean
  onShowSubIssuesChange: (value: boolean) => void
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
  showSubIssues,
  onShowSubIssuesChange,
}: TaskFiltersProps) {
  const activeCount = (statusKey ? 1 : 0) + (assigneeId ? 1 : 0)
  const sortLabel = SORT_OPTIONS.find((o) => o.key === sort)?.label ?? 'Sort'

  return (
    <>
      <label className="tasks-search">
        <SearchNormal size={15} aria-hidden="true" />
        <input type="search" aria-label="Search tasks" placeholder="Search tasks" value={search} onChange={(event) => onSearchChange(event.target.value)} />
      </label>
      <Dropdown
        align="right"
        trigger={() => (
          <button className="button button-ghost tasks-filter" data-active={activeCount > 0 || undefined} aria-label="Filter tasks">
            <Filter size={15} />
            <span className="tasks-filter-label">Filter</span>
            {activeCount > 0 ? <span className="tasks-filter-count">{activeCount}</span> : null}
          </button>
        )}
      >
        {(close) => (
          <>
            <div className="popover-heading">Status</div>
            {groups.map((group) => (
              <button
                key={group.key}
                className="popover-option"
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
            <div className="popover-separator" />
            <div className="popover-heading">Assignee</div>
            {users.map((u) => (
              <button
                key={u.id}
                className="popover-option"
                data-selected={u.id === assigneeId || undefined}
                onClick={() => {
                  onAssigneeChange(u.id === assigneeId ? null : u.id)
                  close()
                }}
              >
                <Avatar user={u} size={16} />
                {u.name}
              </button>
            ))}
            {activeCount > 0 ? (
              <>
                <div className="popover-separator" />
                <button
                  className="popover-option"
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
          </>
        )}
      </Dropdown>

      <Dropdown
        align="right"
        trigger={() => (
          <button className="button button-ghost tasks-filter" data-active={sort !== 'manual' || undefined} aria-label={`Sort tasks: ${sortLabel}`}>
            <Sort size={15} />
            <span className="tasks-filter-label">{sort === 'manual' ? 'Sort' : sortLabel}</span>
          </button>
        )}
      >
        {(close) => (
          <>
            <div className="popover-heading">Sort by</div>
            {SORT_OPTIONS.map((option) => (
              <button
                key={option.key}
                className="popover-option"
                data-selected={option.key === sort || undefined}
                onClick={() => {
                  onSortChange(option.key)
                  close()
                }}
              >
                {option.label}
              </button>
            ))}
          </>
        )}
      </Dropdown>

      <Dropdown
        align="right"
        trigger={() => (
          <button className="button button-ghost tasks-filter" aria-label="Display options">
            <Setting4 size={15} />
            <span className="tasks-filter-label">Display</span>
          </button>
        )}
      >
        {(close) => (
          <>
            <div className="popover-heading">Layout</div>
            <button
              className="popover-option"
              data-selected={layout === 'list' || undefined}
              onClick={() => {
                onLayoutChange('list')
                close()
              }}
            >
              <List size={15} />
              List
            </button>
            <button
              className="popover-option"
              data-selected={layout === 'board' || undefined}
              onClick={() => {
                onLayoutChange('board')
                close()
              }}
            >
              <Kanban size={15} />
              Board
            </button>
            <div className="popover-separator" />
            <div className="popover-heading">Sub-issues</div>
            <button
              className="popover-option"
              role="menuitemcheckbox"
              aria-checked={showSubIssues}
              data-selected={showSubIssues || undefined}
              onClick={() => {
                onShowSubIssuesChange(!showSubIssues)
                close()
              }}
            >
              <Hierarchy size={15} />
              Show sub-issues
            </button>
          </>
        )}
      </Dropdown>
    </>
  )
}
