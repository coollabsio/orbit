import { useState } from 'react'
import { Modal } from '../../../components/ui/Modal'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '../../../components/workspace/taskMeta'
import { createTask } from '../../../mock/actions'
import type { Project, TaskPriority } from '../../../mock/types'

interface NewTaskModalProps {
  projects: Project[]
  defaultProjectId: string | null
  onClose: () => void
  onCreated: (taskId: string) => void
}

export function NewTaskModal({ projects, defaultProjectId, onClose, onCreated }: NewTaskModalProps) {
  const [title, setTitle] = useState('')
  const [projectId, setProjectId] = useState(defaultProjectId ?? projects[0]?.id ?? '')
  const [priority, setPriority] = useState<TaskPriority>('none')

  const submit = () => {
    const trimmed = title.trim()
    if (!trimmed || !projectId) return
    const task = createTask({ title: trimmed, projectId, priority })
    onCreated(task.id)
  }

  return (
    <Modal title="New task" onClose={onClose}>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          submit()
        }}
        style={{ display: 'flex', flexDirection: 'column', gap: 12 }}
      >
        <div>
          <label className="field-label" htmlFor="new-task-title">
            Title
          </label>
          <input
            id="new-task-title"
            className="input"
            placeholder="Task title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            autoFocus
          />
        </div>
        <div>
          <label className="field-label" htmlFor="new-task-project">
            Project
          </label>
          <select
            id="new-task-project"
            className="input"
            value={projectId}
            onChange={(e) => setProjectId(e.target.value)}
          >
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="field-label" htmlFor="new-task-priority">
            Priority
          </label>
          <select
            id="new-task-priority"
            className="input"
            value={priority}
            onChange={(e) => setPriority(e.target.value as TaskPriority)}
          >
            {PRIORITY_ORDER.map((p) => (
              <option key={p} value={p}>
                {PRIORITY_LABEL[p]}
              </option>
            ))}
          </select>
        </div>
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 4 }}>
          <button type="button" className="button button-ghost" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button-primary" disabled={!title.trim()}>
            Create task
          </button>
        </div>
      </form>
    </Modal>
  )
}
