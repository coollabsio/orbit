import { useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { ArrowLeft, TaskSquare, TickCircle } from 'reicon-react'
import { EmptyState } from '../../components/ui/EmptyState'
import { TaskStatusIcon } from '../../components/workspace/TaskStatusIcon'
import { STATUS_LABEL } from '../../components/workspace/taskMeta'
import { deleteProject, updateProject } from '../../mock/actions'
import { useAppState } from '../../mock/store'
import type { TaskStatus } from '../../mock/types'
import { ConfirmDeleteModal } from '../chat/components/ChannelModals'
import { SettingsCard } from '../settings/SettingsCard'
import '../shared/cards.css'
import '../settings/settings.css'
import './tasks.css'

const PROJECT_COLORS = [
  '#8b5cf6', '#6366f1', '#0ea5e9', '#06b6d4', '#10b981', '#22c55e', '#84cc16', '#eab308', '#f59e0b', '#f97316', '#ef4444',
  '#ec4899', '#d946ef', '#64748b', '#78716c',
]

/** Status workflow, grouped like the reference: each group holds one built-in status. */
const STATUS_GROUPS: { label: string; status: TaskStatus; note?: string }[] = [
  { label: 'Unstarted', status: 'todo', note: 'Default' },
  { label: 'Started', status: 'in_progress' },
  { label: 'Completed', status: 'done' },
  { label: 'Cancelled', status: 'cancelled' },
]

/** One-page project configuration (no sub-navigation): general, statuses, danger zone. */
export function ProjectSettingsPage() {
  const { projectId } = useParams()
  const navigate = useNavigate()
  const state = useAppState()
  const project = state.projects.find((p) => p.id === projectId)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const tasks = state.tasks.filter((t) => t.projectId === projectId)
  const countFor = (status: TaskStatus) => tasks.filter((t) => t.status === status).length

  return (
    <div className="page">
      <section className="pane" style={{ flex: 1 }}>
        <div className="pane-header">
          <button className="icon-button" onClick={() => navigate('/tasks')} aria-label="Back to tasks">
            <ArrowLeft size={16} />
          </button>
          <span className="pane-title">{project ? `${project.name} settings` : 'Project settings'}</span>
        </div>
        {!project ? (
          <div className="pane-body">
            <EmptyState icon={TaskSquare} title="Project not found" description="This project does not exist or was removed." />
          </div>
        ) : (
          <div className="settings-scroll">
            <div className="settings-content project-settings">
              <SettingsCard title="General" description="Name, tag and color of this project.">
                <div className="settings-grid">
                  <div className="settings-field">
                    <label className="field-label" htmlFor="project-name">
                      Name
                    </label>
                    <input
                      id="project-name"
                      key={`name-${project.id}`}
                      className="input"
                      defaultValue={project.name}
                      onBlur={(e) => {
                        const name = e.target.value.trim()
                        if (name && name !== project.name) updateProject(project.id, { name })
                      }}
                    />
                  </div>
                  <div className="settings-field">
                    <label className="field-label" htmlFor="project-key">
                      Tag
                    </label>
                    <input
                      id="project-key"
                      key={`key-${project.id}`}
                      className="input"
                      defaultValue={project.key}
                      maxLength={5}
                      placeholder="INF"
                      onBlur={(e) => {
                        const key = e.target.value.trim().toUpperCase().replace(/[^A-Z0-9]/g, '')
                        e.target.value = key
                        if (key && key !== project.key) updateProject(project.id, { key })
                      }}
                    />
                    <p className="settings-help">Short prefix used in task ids, for example {project.key}-101.</p>
                  </div>
                  <div className="settings-field col-span-2">
                    <span className="field-label">Color</span>
                    <div className="ps-swatches">
                      {PROJECT_COLORS.map((color) => (
                        <button
                          key={color}
                          type="button"
                          className="ps-swatch"
                          style={{ backgroundColor: color }}
                          title={color}
                          aria-label={`Color ${color}`}
                          aria-pressed={project.color.toLowerCase() === color}
                          onClick={() => updateProject(project.id, { color })}
                        >
                          {project.color.toLowerCase() === color ? <TickCircle size={14} /> : null}
                        </button>
                      ))}
                      <label className="ps-swatch ps-swatch-custom" style={{ backgroundColor: project.color }} title="Custom color">
                        <input
                          type="color"
                          value={project.color}
                          aria-label="Custom color"
                          onChange={(e) => updateProject(project.id, { color: e.target.value })}
                        />
                      </label>
                    </div>
                  </div>
                </div>
              </SettingsCard>

              <SettingsCard title="Statuses" description="The workflow a task goes through from start to completion." flush>
                <div className="ps-status-list">
                  {STATUS_GROUPS.map((group) => {
                    const count = countFor(group.status)
                    return (
                      <div key={group.status} className="ps-status-group">
                        <div className="ps-status-group-header">{group.label}</div>
                        <div className="ps-status-row">
                          <span className="ps-status-tile">
                            <TaskStatusIcon status={group.status} size={16} />
                          </span>
                          <span className="ps-status-text">
                            <span className="ps-status-name">
                              {STATUS_LABEL[group.status]}
                              {group.note ? <span className="ps-status-note"> · {group.note}</span> : null}
                            </span>
                            {count > 0 ? (
                              <span className="ps-status-count">
                                {count} {count === 1 ? 'task' : 'tasks'}
                              </span>
                            ) : null}
                          </span>
                        </div>
                      </div>
                    )
                  })}
                </div>
              </SettingsCard>

              <SettingsCard title="Danger zone" description="Deleting a project removes the project and all of its tasks.">
                <button type="button" className="button button-danger" onClick={() => setConfirmDelete(true)}>
                  Delete project
                </button>
              </SettingsCard>
            </div>
          </div>
        )}
      </section>

      {confirmDelete && project ? (
        <ConfirmDeleteModal
          title="Delete project?"
          description={`This will permanently delete "${project.name}" and its ${tasks.length} ${tasks.length === 1 ? 'task' : 'tasks'}.`}
          onClose={() => setConfirmDelete(false)}
          onConfirm={() => {
            deleteProject(project.id)
            navigate('/tasks')
          }}
        />
      ) : null}
    </div>
  )
}
