import { useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { Add, ArrowLeft, Edit, More, TaskSquare, TickCircle, Trash } from 'reicon-react'
import { Dropdown } from '../../components/ui/Dropdown'
import { EmptyState } from '../../components/ui/EmptyState'
import { TaskStatusIcon } from '../../components/workspace/TaskStatusIcon'
import { CATEGORY_LABEL, CATEGORY_ORDER, STATUS_COLORS, defaultStatusOf, projectStatuses } from '../../components/workspace/taskMeta'
import { createStatus, deleteProject, deleteStatus, reorderStatus, updateProject, updateStatus } from '../../mock/actions'
import { useAppState } from '../../mock/store'
import type { StatusCategory, TaskStatusDef } from '../../mock/types'
import { ConfirmDeleteModal } from '../chat/components/ChannelModals'
import { SettingsCard } from '../settings/SettingsCard'
import '../shared/cards.css'
import '../settings/settings.css'
import './tasks.css'

const PROJECT_COLORS = [
  '#8b5cf6', '#6366f1', '#0ea5e9', '#06b6d4', '#10b981', '#22c55e', '#84cc16', '#eab308', '#f59e0b', '#f97316', '#ef4444',
  '#ec4899', '#d946ef', '#64748b', '#78716c',
]

type Editor = { mode: 'new'; category: StatusCategory } | { mode: 'edit'; statusId: string }

/** One-page project configuration (no sub-navigation): general, statuses, danger zone. */
export function ProjectSettingsPage() {
  const { projectId } = useParams()
  const navigate = useNavigate()
  const state = useAppState()
  const project = state.projects.find((p) => p.id === projectId)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [editor, setEditor] = useState<Editor | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<TaskStatusDef | null>(null)
  const [dragId, setDragId] = useState<string | null>(null)
  const [dropAt, setDropAt] = useState<{ id: string; position: 'before' | 'after' } | null>(null)

  const tasks = state.tasks.filter((t) => t.projectId === projectId)
  const statuses = project ? projectStatuses(state.statuses, project.id) : []
  const defaultStatus = project ? defaultStatusOf(state.statuses, project.id) : undefined
  const countFor = (statusId: string) => tasks.filter((t) => t.statusId === statusId).length

  const endDrag = () => {
    setDragId(null)
    setDropAt(null)
  }

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
                  {CATEGORY_ORDER.map((category) => {
                    const own = statuses.filter((s) => s.category === category)
                    return (
                      <div key={category} className="ps-status-group">
                        <div className="ps-status-group-header">
                          <span>{CATEGORY_LABEL[category]}</span>
                          <button
                            type="button"
                            className="icon-button ps-status-add"
                            aria-label={`Add ${CATEGORY_LABEL[category].toLowerCase()} status`}
                            title="Add status"
                            onClick={() => setEditor({ mode: 'new', category })}
                          >
                            <Add size={14} />
                          </button>
                        </div>
                        {own.map((status) =>
                          editor?.mode === 'edit' && editor.statusId === status.id ? (
                            <StatusEditor
                              key={status.id}
                              category={status.category}
                              initial={status}
                              onCancel={() => setEditor(null)}
                              onSave={(values) => {
                                updateStatus(status.id, values)
                                setEditor(null)
                              }}
                            />
                          ) : (
                            <div
                              key={status.id}
                              className="ps-status-row"
                              draggable
                              data-dragging={dragId === status.id || undefined}
                              data-drop={dropAt?.id === status.id ? dropAt.position : undefined}
                              onDragStart={(e) => {
                                e.dataTransfer.effectAllowed = 'move'
                                e.dataTransfer.setData('text/status-id', status.id)
                                setDragId(status.id)
                              }}
                              onDragEnd={endDrag}
                              onDragOver={(e) => {
                                // reorder only inside the same category
                                const dragged = dragId ? state.statuses.find((s) => s.id === dragId) : undefined
                                if (!dragged || dragged.id === status.id || dragged.category !== status.category) return
                                e.preventDefault()
                                const rect = e.currentTarget.getBoundingClientRect()
                                const position = e.clientY < rect.top + rect.height / 2 ? 'before' : 'after'
                                if (dropAt?.id !== status.id || dropAt.position !== position) setDropAt({ id: status.id, position })
                              }}
                              onDragLeave={(e) => {
                                if (!e.currentTarget.contains(e.relatedTarget as Node | null) && dropAt?.id === status.id) setDropAt(null)
                              }}
                              onDrop={(e) => {
                                e.preventDefault()
                                const id = e.dataTransfer.getData('text/status-id') || dragId
                                if (id && dropAt?.id === status.id) reorderStatus(id, status.id, dropAt.position)
                                endDrag()
                              }}
                            >
                              <span className="ps-status-grip" aria-hidden="true">
                                <GripIcon />
                              </span>
                              <span className="ps-status-tile">
                                <TaskStatusIcon status={status} size={16} />
                              </span>
                              <span className="ps-status-text">
                                <span className="ps-status-name">
                                  {status.name}
                                  {status.id === defaultStatus?.id ? <span className="ps-status-note"> · Default</span> : null}
                                </span>
                                {status.description ? (
                                  <span className="ps-status-count">{status.description}</span>
                                ) : countFor(status.id) > 0 ? (
                                  <span className="ps-status-count">
                                    {countFor(status.id)} {countFor(status.id) === 1 ? 'task' : 'tasks'}
                                  </span>
                                ) : null}
                              </span>
                              <div className="spacer" />
                              <Dropdown
                                align="right"
                                trigger={() => (
                                  <button className="icon-button ps-status-menu" aria-label={`${status.name} actions`} title="Actions">
                                    <More size={16} />
                                  </button>
                                )}
                              >
                                {(close) => (
                                  <>
                                    <button
                                      className="popover-option"
                                      onClick={() => {
                                        setEditor({ mode: 'edit', statusId: status.id })
                                        close()
                                      }}
                                    >
                                      <Edit size={15} />
                                      Edit
                                    </button>
                                    <button
                                      className="popover-option"
                                      data-tone="danger"
                                      disabled={statuses.length === 1}
                                      onClick={() => {
                                        setDeleteTarget(status)
                                        close()
                                      }}
                                    >
                                      <Trash size={15} />
                                      Delete
                                    </button>
                                  </>
                                )}
                              </Dropdown>
                            </div>
                          ),
                        )}
                        {editor?.mode === 'new' && editor.category === category ? (
                          <StatusEditor
                            category={category}
                            onCancel={() => setEditor(null)}
                            onSave={(values) => {
                              createStatus(project.id, { ...values, category })
                              setEditor(null)
                            }}
                          />
                        ) : null}
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

      {deleteTarget && project ? (
        <ConfirmDeleteModal
          title={`Delete "${deleteTarget.name}"?`}
          description={
            countFor(deleteTarget.id) > 0
              ? `${countFor(deleteTarget.id)} ${countFor(deleteTarget.id) === 1 ? 'task uses' : 'tasks use'} this status. They will move to the next status of the project.`
              : 'This status is not used by any task.'
          }
          onClose={() => setDeleteTarget(null)}
          onConfirm={() => {
            deleteStatus(deleteTarget.id)
            setDeleteTarget(null)
          }}
        />
      ) : null}

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

/** Inline row to create or edit a status: color tile (palette), name, description, Cancel / Save. */
function StatusEditor({
  category,
  initial,
  onCancel,
  onSave,
}: {
  category: StatusCategory
  initial?: TaskStatusDef
  onCancel: () => void
  onSave: (values: { name: string; description: string; color: string }) => void
}) {
  const [name, setName] = useState(initial?.name ?? '')
  const [description, setDescription] = useState(initial?.description ?? '')
  const [color, setColor] = useState(initial?.color ?? STATUS_COLORS[0])
  const canSave = name.trim().length > 0

  return (
    <form
      className="ps-status-editor"
      onSubmit={(e) => {
        e.preventDefault()
        if (canSave) onSave({ name: name.trim(), description: description.trim(), color })
      }}
    >
      <Dropdown
        trigger={() => (
          <button type="button" className="ps-status-tile ps-status-tile-button" aria-label="Status color" title="Color">
            <TaskStatusIcon status={{ category, color }} size={16} />
          </button>
        )}
      >
        {() => (
          <div className="ps-color-palette">
            {STATUS_COLORS.map((preset) => (
              <button
                key={preset}
                type="button"
                className="ps-color-dot"
                style={{ backgroundColor: preset }}
                aria-label={`Color ${preset}`}
                aria-pressed={color === preset}
                onClick={() => setColor(preset)}
              >
                {color === preset ? <TickCircle size={14} /> : null}
              </button>
            ))}
            <span className="ps-color-sep" />
            <label className="ps-color-dot ps-color-custom" title="Custom color">
              <input type="color" value={color} aria-label="Custom color" onChange={(e) => setColor(e.target.value)} />
            </label>
          </div>
        )}
      </Dropdown>
      <input
        className="input ps-status-input"
        placeholder="Status name"
        aria-label="Status name"
        value={name}
        autoFocus
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') onCancel()
        }}
      />
      <input
        className="input ps-status-input ps-status-input-desc"
        placeholder="Description…"
        aria-label="Description"
        value={description}
        onChange={(e) => setDescription(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') onCancel()
        }}
      />
      <button type="button" className="button button-ghost" onClick={onCancel}>
        Cancel
      </button>
      <button type="submit" className="button button-primary" disabled={!canSave}>
        Save
      </button>
    </form>
  )
}

function GripIcon() {
  return (
    <svg width="10" height="14" viewBox="0 0 10 14" fill="currentColor" aria-hidden="true">
      <circle cx="3" cy="2.5" r="1.2" />
      <circle cx="7" cy="2.5" r="1.2" />
      <circle cx="3" cy="7" r="1.2" />
      <circle cx="7" cy="7" r="1.2" />
      <circle cx="3" cy="11.5" r="1.2" />
      <circle cx="7" cy="11.5" r="1.2" />
    </svg>
  )
}
