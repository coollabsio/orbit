import { Avatar } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { STATUS_LABEL, STATUS_ORDER } from '../../../components/workspace/taskMeta'
import { cx } from '../../../lib/cx'
import type { Project, TaskStatus, User } from '../../../mock/types'

interface TaskFiltersProps {
  projects: Project[]
  users: User[]
  projectId: string | null
  status: TaskStatus | null
  assigneeId: string | null
  onProjectChange: (projectId: string | null) => void
  onStatusChange: (status: TaskStatus | null) => void
  onAssigneeChange: (assigneeId: string | null) => void
}

export function TaskFilters({
  projects,
  users,
  projectId,
  status,
  assigneeId,
  onProjectChange,
  onStatusChange,
  onAssigneeChange,
}: TaskFiltersProps) {
  const project = projects.find((p) => p.id === projectId)
  const assignee = users.find((u) => u.id === assigneeId)
  const anyActive = Boolean(projectId || status || assigneeId)

  return (
    <div className="pane-toolbar">
      <Dropdown
        trigger={() => (
          <button className={cx('button', 'button-ghost', 'tasks-filter')} data-active={Boolean(project) || undefined}>
            {project ? (
              <>
                <span className="pill-dot" style={{ background: project.color }} />
                {project.name}
              </>
            ) : (
              'Project'
            )}
          </button>
        )}
      >
        {(close) => (
          <>
            <div className="popover-heading">Project</div>
            {projects.map((p) => (
              <button
                key={p.id}
                className="popover-option"
                data-selected={p.id === projectId || undefined}
                onClick={() => {
                  onProjectChange(p.id === projectId ? null : p.id)
                  close()
                }}
              >
                <span className="pill-dot" style={{ background: p.color }} />
                {p.name}
              </button>
            ))}
            {projectId ? (
              <>
                <div className="popover-separator" />
                <button
                  className="popover-option"
                  onClick={() => {
                    onProjectChange(null)
                    close()
                  }}
                >
                  Clear project
                </button>
              </>
            ) : null}
          </>
        )}
      </Dropdown>

      <Dropdown
        trigger={() => (
          <button className={cx('button', 'button-ghost', 'tasks-filter')} data-active={Boolean(status) || undefined}>
            {status ? (
              <>
                <TaskStatusIcon status={status} />
                {STATUS_LABEL[status]}
              </>
            ) : (
              'Status'
            )}
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
            {status ? (
              <>
                <div className="popover-separator" />
                <button
                  className="popover-option"
                  onClick={() => {
                    onStatusChange(null)
                    close()
                  }}
                >
                  Clear status
                </button>
              </>
            ) : null}
          </>
        )}
      </Dropdown>

      <Dropdown
        trigger={() => (
          <button className={cx('button', 'button-ghost', 'tasks-filter')} data-active={Boolean(assignee) || undefined}>
            {assignee ? (
              <>
                <Avatar user={assignee} size={16} />
                {assignee.name}
              </>
            ) : (
              'Assignee'
            )}
          </button>
        )}
      >
        {(close) => (
          <>
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
            {assigneeId ? (
              <>
                <div className="popover-separator" />
                <button
                  className="popover-option"
                  onClick={() => {
                    onAssigneeChange(null)
                    close()
                  }}
                >
                  Clear assignee
                </button>
              </>
            ) : null}
          </>
        )}
      </Dropdown>

      {anyActive ? (
        <>
          <div className="spacer" />
          <button
            className="button button-ghost"
            onClick={() => {
              onProjectChange(null)
              onStatusChange(null)
              onAssigneeChange(null)
            }}
          >
            Clear filters
          </button>
        </>
      ) : null}
    </div>
  )
}
