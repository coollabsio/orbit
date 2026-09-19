import { useMemo, useState } from 'react'
import { Add } from 'reicon-react'
import { useWorkspace } from '../../workspaces/workspaceContext'
import type { Project, Task, TaskViewState } from '../api/models'
import { taskFromRecord } from '../api/models'
import { useSubIssues } from '../api/tasks'
import { NewTaskModal } from './NewTaskModal'
import { TaskRow } from './TaskRow'

interface SubIssuesProps {
  task: Task
  projects: Project[]
  state: TaskViewState
  onOpen: (taskId: string) => void
}

/** Sub-issues of one task: count, progress, compact rows, and creation through the shared modal. */
export function SubIssues({ task, projects, state, onOpen }: SubIssuesProps) {
  const { workspace } = useWorkspace()
  const childrenQuery = useSubIssues(workspace.id, task.id, task.subIssueTotal > 0)
  const [adding, setAdding] = useState(false)

  const records = childrenQuery.data?.items
  const children = useMemo(() => (records ?? []).map((record) => taskFromRecord(record)), [records])
  const total = task.subIssueTotal
  const done = Math.min(task.subIssueDone, total)
  const percent = total > 0 ? Math.round((done / total) * 100) : 0
  // With nothing to show, drop the titled section for one quiet action, like Linear.
  const showFull = total > 0 || children.length > 0

  return (
    <section className="tasks-subissues" data-empty={!showFull || undefined} aria-label="Sub-issues">
      {showFull ? (
        <div className="tasks-subissues-header">
          <span className="tasks-subissues-title">Sub-issues</span>
          {total > 0 ? (
            <>
              <span className="tasks-subissues-count">{done}/{total}</span>
              <div
                className="tasks-subissues-bar"
                role="progressbar"
                aria-label="Sub-issue progress"
                aria-valuenow={done}
                aria-valuemin={0}
                aria-valuemax={total}
              >
                <span className="tasks-subissues-bar-fill" style={{ width: `${percent}%` }} />
              </div>
            </>
          ) : null}
          <div className="spacer" />
          <button
            type="button"
            className="icon-button tasks-subissues-add"
            aria-label="Add sub-issue"
            title="Add sub-issue"
            onClick={() => setAdding(true)}
          >
            <Add size={14} />
          </button>
        </div>
      ) : (
        <button
          type="button"
          className="button button-ghost tasks-subissues-empty-add"
          onClick={() => setAdding(true)}
        >
          <Add size={14} />
          Add sub-issue
        </button>
      )}
      {children.length > 0 ? (
        <div className="tasks-subissues-list">
          {children.map((child) => {
            const project = projects.find((item) => item.id === child.projectId)
            return (
              <TaskRow
                key={child.id}
                task={child}
                labels={state.labels}
                statuses={state.statuses}
                users={state.users}
                assignees={state.users.filter((user) => child.assigneeIds.includes(user.id))}
                selected={false}
                dragging={false}
                compact
                // Cross-project children carry their own project colour.
                projectColor={project && project.id !== task.projectId ? project.color : undefined}
                onOpen={onOpen}
                onToggleSelect={() => {}}
                onDragStart={() => {}}
                onDragEnd={() => {}}
              />
            )
          })}
        </div>
      ) : null}
      {childrenQuery.isError ? <p role="alert" className="text-danger text-xs">Sub-issues could not be loaded.</p> : null}
      {adding ? (
        <NewTaskModal
          workspaceId={workspace.id}
          projects={projects}
          statuses={state.statuses}
          users={state.users}
          labels={state.labels}
          defaultProjectId={task.projectId}
          parent={task}
          onClose={() => setAdding(false)}
        />
      ) : null}
    </section>
  )
}
