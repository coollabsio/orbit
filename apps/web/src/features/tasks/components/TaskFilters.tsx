import { Filter, Kanban, List, Setting4, Sort } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import type { User } from '../../../mock/types'
import { SORT_OPTIONS, type SortKey, type StatusGroup } from '../tasksLib'

interface TaskFiltersProps {
  users: User[]
  groups: StatusGroup[]
  statusKey: string | null
  assigneeId: string | null
  sort: SortKey
  layout: 'list' | 'board'
  onStatusChange: (key: string | null) => void
  onAssigneeChange: (assigneeId: string | null) => void
  onSortChange: (sort: SortKey) => void
  onLayoutChange: (layout: 'list' | 'board') => void
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
}: TaskFiltersProps) {
  const activeCount = (statusKey ? 1 : 0) + (assigneeId ? 1 : 0)
  const sortLabel = SORT_OPTIONS.find((o) => o.key === sort)?.label ?? 'Sort'

  return (
    <>
      <Dropdown
        align="right"
        trigger={() => (
          <button className="button button-ghost tasks-filter" data-active={activeCount > 0 || undefined}>
            <Filter size={15} />
            Filter
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
          <button className="button button-ghost tasks-filter" data-active={sort !== 'manual' || undefined}>
            <Sort size={15} />
            {sort === 'manual' ? 'Sort' : sortLabel}
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
          <button className="button button-ghost tasks-filter">
            <Setting4 size={15} />
            Display
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
          </>
        )}
      </Dropdown>
    </>
  )
}
