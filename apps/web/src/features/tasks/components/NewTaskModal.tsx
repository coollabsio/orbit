import { useMemo, useRef, useState } from 'react'
import { Calendar, ChevronDown, Xmark } from 'reicon-react'
import { Modal } from '../../../components/ui/Modal'
import { Dropdown } from '../../../components/ui/Dropdown'
import { DatePicker } from '../../../components/ui/DatePicker'
import { Avatar, AvatarStack } from '../../../components/ui/Avatar'
import { PriorityIcon } from '../../../components/workspace/PriorityIcon'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER, defaultStatusOf, projectStatuses } from '../../../components/workspace/taskMeta'
import { RichTextEditor } from '../../../components/editor/RichTextEditor'
import { EMPTY_DOCUMENT, isEmptyDocument, type RichTextDocument } from '../../../components/editor/document'
import { useWorkspace } from '../../workspaces/workspaceContext'
import type { LabelRecord } from '../../../api/generated/types.gen'
import type { Project, Task, TaskPriority, TaskStatusDef, User } from '../api/models'
import { useCreateTask } from '../api/tasks'
import { resolveStatusId } from '../tasksLib'
import { TaskLabels } from './TaskLabels'

interface NewTaskModalProps {
  workspaceId: string
  projects: Project[]
  /** Every status across every project; the modal narrows them to the chosen project. */
  statuses: TaskStatusDef[]
  users: User[]
  labels: LabelRecord[]
  /** Preselects the project (falls back to the parent's project, then the first project). */
  defaultProjectId?: string | null
  /** Preselects the status by its category key (from a list group "+"). */
  defaultStatusKey?: string | null
  /** When set the new task is a sub-issue of this task. */
  parent?: Task
  onClose: () => void
  /** The created task, so callers can refresh the right list. */
  onCreated?: (task: { id: string }) => void
}

function dueLabel(value: string | null) {
  if (!value) return 'Due date'
  return new Date(value).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

/** Linear-style create dialog: fill everything in one place, no navigation on create. */
export function NewTaskModal({
  workspaceId,
  projects,
  statuses,
  users,
  labels,
  defaultProjectId,
  defaultStatusKey,
  parent,
  onClose,
  onCreated,
}: NewTaskModalProps) {
  const { workspace } = useWorkspace()
  const createTask = useCreateTask(workspaceId)

  const initialProjectId = defaultProjectId ?? parent?.projectId ?? projects[0]?.id ?? ''
  const [projectId, setProjectId] = useState(initialProjectId)
  const options = useMemo(() => projectStatuses(statuses, projectId), [statuses, projectId])
  const [statusId, setStatusId] = useState(() => resolveStatusId(statuses, initialProjectId, defaultStatusKey ?? null) ?? '')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState<RichTextDocument>(EMPTY_DOCUMENT)
  const [priority, setPriority] = useState<TaskPriority>('none')
  const [assigneeIds, setAssigneeIds] = useState<string[]>([])
  const [labelIds, setLabelIds] = useState<string[]>([])
  const [dueAt, setDueAt] = useState<string | null>(null)
  const [createMore, setCreateMore] = useState(false)
  // Bumped after a "create more" so the description editor remounts empty.
  const [generation, setGeneration] = useState(0)
  const busy = useRef(false)

  const project = projects.find((item) => item.id === projectId)
  const status = options.find((item) => item.id === statusId)
  const assignees = users.filter((user) => assigneeIds.includes(user.id))

  const chooseProject = (id: string) => {
    setProjectId(id)
    // The old status belongs to the old project; fall back to the new project's default.
    if (!projectStatuses(statuses, id).some((item) => item.id === statusId)) {
      setStatusId(defaultStatusOf(statuses, id)?.id ?? '')
    }
  }

  const submit = async () => {
    const trimmed = title.trim()
    if (!trimmed || !projectId || !statusId || busy.current || createTask.isPending) return
    busy.current = true
    try {
      const task = await createTask.mutateAsync({
        title: trimmed,
        project_id: projectId,
        status_id: statusId,
        priority,
        parent_id: parent?.id ?? null,
        assignee_ids: assigneeIds,
        label_ids: labelIds,
        due_at: dueAt,
        description_json: isEmptyDocument(description) ? undefined : description,
      })
      onCreated?.(task)
      if (createMore) {
        // Keep project/status/priority so a burst of related issues is quick to enter.
        setTitle('')
        setDescription(EMPTY_DOCUMENT)
        setAssigneeIds([])
        setLabelIds([])
        setDueAt(null)
        setGeneration((current) => current + 1)
      } else {
        onClose()
      }
    } catch {
      // The failure is reported below the fields; the draft stays for a retry.
    } finally {
      busy.current = false
    }
  }

  const canCreate = title.trim().length > 0 && !!projectId && !!statusId && !createTask.isPending

  return (
    <Modal
      title={parent ? 'New sub-issue' : 'New issue'}
      description={parent ? `Sub-issue of ${parent.identifier} · ${parent.title}` : workspace.name}
      onClose={onClose}
      maxWidth={640}
    >
      {/* A div, not a <form>: the label picker renders its own form, and Cmd/Ctrl+Enter
          (not a bare Enter in the title) is the deliberate submit, like Linear. */}
      <div
        className="tasks-new"
        onKeyDown={(event) => {
          if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
            event.preventDefault()
            void submit()
          }
        }}
      >
        <input
          className="tasks-new-title"
          value={title}
          placeholder="Issue title"
          aria-label="Issue title"
          autoFocus
          disabled={createTask.isPending}
          onChange={(event) => setTitle(event.target.value)}
        />
        <RichTextEditor
          key={generation}
          value={EMPTY_DOCUMENT}
          placeholder="Add description…"
          ariaLabel="Description"
          workspaceId={workspaceId}
          members={users}
          statuses={statuses}
          onChange={setDescription}
          onSubmit={() => void submit()}
        />

        <div className="tasks-new-props">
          <Dropdown
            trigger={() => (
              <button type="button" className="button button-ghost tasks-new-prop">
                <TaskStatusIcon status={status} />
                {status?.name ?? 'Status'}
              </button>
            )}
          >
            {(close) => (
              <>
                {options.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    className="popover-option"
                    data-selected={option.id === statusId || undefined}
                    onClick={() => { setStatusId(option.id); close() }}
                  >
                    <TaskStatusIcon status={option} />
                    {option.name}
                  </button>
                ))}
              </>
            )}
          </Dropdown>

          <Dropdown
            trigger={() => (
              <button type="button" className="button button-ghost tasks-new-prop">
                <PriorityIcon priority={priority} />
                {priority === 'none' ? 'Priority' : PRIORITY_LABEL[priority]}
              </button>
            )}
          >
            {(close) => (
              <>
                {PRIORITY_ORDER.map((value) => (
                  <button
                    key={value}
                    type="button"
                    className="popover-option"
                    data-selected={value === priority || undefined}
                    onClick={() => { setPriority(value); close() }}
                  >
                    <PriorityIcon priority={value} />
                    {PRIORITY_LABEL[value]}
                  </button>
                ))}
              </>
            )}
          </Dropdown>

          <Dropdown
            trigger={() => (
              <button type="button" className="button button-ghost tasks-new-prop">
                {assignees.length > 0 ? (
                  <>
                    <AvatarStack users={assignees} size={16} />
                    <span className="truncate">{assignees.map((user) => user.name).join(', ')}</span>
                  </>
                ) : (
                  <>
                    <Avatar user={undefined} size={16} name="—" />
                    Assign
                  </>
                )}
              </button>
            )}
          >
            {() => (
              <>
                <div className="popover-heading">Assignees</div>
                {users.map((user) => {
                  const active = assigneeIds.includes(user.id)
                  return (
                    <button
                      key={user.id}
                      type="button"
                      className="popover-option"
                      data-selected={active || undefined}
                      aria-pressed={active}
                      onClick={() => setAssigneeIds(active ? assigneeIds.filter((id) => id !== user.id) : [...assigneeIds, user.id])}
                    >
                      <Avatar user={user} size={16} />
                      {user.name}
                      {active ? <Xmark size={14} className="popover-option-remove" aria-hidden="true" /> : null}
                    </button>
                  )
                })}
              </>
            )}
          </Dropdown>

          <TaskLabels
            workspaceId={workspaceId}
            labelIds={labelIds}
            labels={labels}
            onChange={setLabelIds}
          />

          <Dropdown
            trigger={() => (
              <button type="button" className="button button-ghost tasks-new-prop">
                {project ? <span className="pill-dot" style={{ background: project.color }} /> : null}
                {project?.name ?? 'Project'}
                <ChevronDown size={13} />
              </button>
            )}
          >
            {(close) => (
              <>
                {projects.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    className="popover-option"
                    data-selected={option.id === projectId || undefined}
                    onClick={() => { chooseProject(option.id); close() }}
                  >
                    <span className="pill-dot" style={{ background: option.color }} />
                    {option.name}
                  </button>
                ))}
              </>
            )}
          </Dropdown>

          <Dropdown
            trigger={() => (
              <button type="button" className="button button-ghost tasks-new-prop" data-active={dueAt ? 'true' : undefined}>
                <Calendar size={15} aria-hidden="true" />
                {dueLabel(dueAt)}
              </button>
            )}
          >
            {(close) => (
              <DatePicker
                value={dueAt}
                onChange={setDueAt}
                onClear={() => { setDueAt(null); close() }}
                onDone={close}
              />
            )}
          </Dropdown>
        </div>

        {createTask.isError ? (
          <p role="alert" className="text-danger text-xs">Task creation failed. Try again.</p>
        ) : null}

        <div className="modal-footer tasks-new-footer">
          <label className="tasks-new-more">
            <input type="checkbox" checked={createMore} onChange={(event) => setCreateMore(event.target.checked)} />
            Create more
          </label>
          <span className="spacer" />
          <button type="button" className="button" disabled={createTask.isPending} onClick={onClose}>Cancel</button>
          <button type="button" className="button button-primary" disabled={!canCreate} onClick={() => void submit()}>
            {createTask.isPending ? 'Creating…' : parent ? 'Create sub-issue' : 'Create issue'}
          </button>
        </div>
      </div>
    </Modal>
  )
}
