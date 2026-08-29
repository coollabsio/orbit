import { Filter, Kanban, List, Setting4 } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { STATUS_LABEL, STATUS_ORDER } from '../../../components/workspace/taskMeta'
import type { TaskStatus, User } from '../../../mock/types'

interface TaskFiltersProps {
  users: User[]
  status: TaskStatus | null
  assigneeId: string | null
  layout: 'list' | 'board'
  onStatusChange: (status: TaskStatus | null) => void
  onAssigneeChange: (assigneeId: string | null) => void
  onLayoutChange: (layout: 'list' | 'board') => void
}

/** Header dropdowns: one "Filter" menu (status + assignee) and one "Display" menu (layout). */
export function TaskFilters({
  users,
  status,
  assigneeId,
  layout,
  onStatusChange,
  onAssigneeChange,
  onLayoutChange,
}: TaskFiltersProps) {
  const activeCount = (status ? 1 : 0) + (assigneeId ? 1 : 0)

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
            {STATUS_ORDER.map((s) => (
              <button
                key={s}
                className="popover-option"
                data-selected={s === status || undefined}
                onClick={() => {
                  onStatusChange(s === status ? null : s)
                  close()
                }}
              >
                <TaskStatusIcon status={s} />
                {STATUS_LABEL[s]}
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
