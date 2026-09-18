import { useEffect, useMemo, useRef, useState } from 'react'
import { Add } from 'reicon-react'
import { defaultStatusOf } from '../../../components/workspace/taskMeta'
import { useWorkspace } from '../../workspaces/workspaceContext'
import type { Project, Task, TaskViewState } from '../api/models'
import { taskFromRecord } from '../api/models'
import { useCreateTask, useSubIssues } from '../api/tasks'
import { TaskRow } from './TaskRow'

interface SubIssuesProps {
  task: Task
  projects: Project[]
  state: TaskViewState
  onOpen: (taskId: string) => void
}

/** Sub-issues of one task: count, progress, compact rows, and inline creation (no modal). */
export function SubIssues({ task, projects, state, onOpen }: SubIssuesProps) {
  const { workspace } = useWorkspace()
  const childrenQuery = useSubIssues(workspace.id, task.id, task.subIssueTotal > 0)
  const createTask = useCreateTask(workspace.id)
  const [draft, setDraft] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const drafting = draft !== null

  useEffect(() => {
    if (drafting) inputRef.current?.focus()
  }, [drafting])

  const records = childrenQuery.data?.items
  const children = useMemo(() => (records ?? []).map((record) => taskFromRecord(record)), [records])
  const total = task.subIssueTotal
  const done = Math.min(task.subIssueDone, total)
  const percent = total > 0 ? Math.round((done / total) * 100) : 0

  const submit = () => {
    const title = draft?.trim()
    const statusId = defaultStatusOf(state.statuses, task.projectId)?.id
    if (!title || !statusId || createTask.isPending) return
    createTask.mutate(
      { title, project_id: task.projectId, status_id: statusId, parent_id: task.id },
      // Stay in the row so the next sub-issue can be typed straight away.
      { onSuccess: () => setDraft('') },
    )
  }

  return (
    <section className="tasks-subissues" aria-label="Sub-issues">
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
          onClick={() => setDraft((current) => current ?? '')}
        >
          <Add size={14} />
        </button>
      </div>
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
      {drafting ? (
        <div className="tasks-subissue-row tasks-subissue-draft">
          <input
            ref={inputRef}
            aria-label="New sub-issue title"
            placeholder="Sub-issue title"
            value={draft}
            disabled={createTask.isPending}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                submit()
              }
              if (event.key === 'Escape') {
                // Keep the page-level Escape (close task) from firing too.
                event.stopPropagation()
                setDraft(null)
              }
            }}
            onBlur={() => {
              if (!draft.trim() && !createTask.isPending) setDraft(null)
            }}
          />
        </div>
      ) : null}
      {createTask.isError ? (
        <p role="alert" className="text-danger text-xs">
          Sub-issue creation failed.{' '}
          <button type="button" className="button button-ghost" onClick={submit}>Retry</button>
        </p>
      ) : null}
    </section>
  )
}
