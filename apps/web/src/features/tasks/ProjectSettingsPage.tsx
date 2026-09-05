import { useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { Add, ArrowLeft, Edit, More, TaskSquare, TickCircle, Trash } from 'reicon-react'
import { Dropdown } from '../../components/ui/Dropdown'
import { EmptyState } from '../../components/ui/EmptyState'
import { InfoTip } from '../../components/ui/InfoTip'
import { UnsavedBar } from '../../components/ui/UnsavedBar'
import { TaskStatusIcon } from '../../components/workspace/TaskStatusIcon'
import { CATEGORY_LABEL, CATEGORY_ORDER, STATUS_COLORS, defaultStatusOf, projectStatuses } from '../../components/workspace/taskMeta'
import { useWorkspace } from '../workspaces/workspaceContext'
import type { Project, StatusCategory, TaskStatusDef } from './api/models'
import { useCreateStatus, useDeleteProject, useDeleteStatus, useProjectStatuses, useProjects, useReorderStatuses, useUpdateProject, useUpdateStatus } from './api/projects'
import { useTasks } from './api/tasks'
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
  const { workspace } = useWorkspace()
  const projectsQuery = useProjects(workspace.id)
  const statusQuery = useProjectStatuses(workspace.id, projectId)
  const tasksQuery = useTasks(workspace.id, { project_id: projectId, limit: 100 })
  const createStatus = useCreateStatus(workspace.id, projectId ?? '')
  const updateStatus = useUpdateStatus(workspace.id, projectId ?? '')
  const deleteStatus = useDeleteStatus(workspace.id, projectId ?? '')
  const reorderStatuses = useReorderStatuses(workspace.id, projectId ?? '')
  const deleteProject = useDeleteProject(workspace.id)
  const project = projectsQuery.data?.find((p) => p.id === projectId)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [editor, setEditor] = useState<Editor | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<TaskStatusDef | null>(null)
  const [dragId, setDragId] = useState<string | null>(null)
  const [dropAt, setDropAt] = useState<{ id: string; position: 'before' | 'after' } | null>(null)

  const tasks = tasksQuery.data?.pages.flatMap((page) => page.items) ?? []
  const statuses = project ? projectStatuses(statusQuery.data ?? [], project.id) : []
  const defaultStatus = project ? defaultStatusOf(statusQuery.data ?? [], project.id) : undefined
  const countFor = (statusId: string) => tasks.filter((t) => t.status_id === statusId).length

  const endDrag = () => {
    setDragId(null)
    setDropAt(null)
  }

  if (projectsQuery.isPending || statusQuery.isPending || tasksQuery.isPending) {
    return <div className="page"><section className="pane" style={{ flex: 1 }}><EmptyState icon={TaskSquare} title="Loading project" description="Loading persisted project settings." /></section></div>
  }
  if (projectsQuery.isError || statusQuery.isError || tasksQuery.isError) {
    return <div className="page"><section className="pane" style={{ flex: 1 }}><EmptyState icon={TaskSquare} title="Project unavailable" description="The server could not load this project." /></section></div>
  }

  return (
    <div className="page">
      <section className="pane project-settings-pane">
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
              <ProjectGeneralCard key={`${project.id}:${project.name}:${project.key}:${project.color}`} project={project} />

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
                                updateStatus.mutate({ statusId: status.id, body: { ...values, category: status.category, position: status.position, expected_version: status.version } })
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
                                const dragged = dragId ? statuses.find((s) => s.id === dragId) : undefined
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
                                if (id && dropAt?.id === status.id) {
                                  const ordered = own.filter((item) => item.id !== id)
                                  const targetIndex = ordered.findIndex((item) => item.id === status.id)
                                  const insertAt = targetIndex + (dropAt.position === 'after' ? 1 : 0)
                                  const dragged = own.find((item) => item.id === id)
                                  if (dragged) {
                                    ordered.splice(insertAt, 0, dragged)
                                    reorderStatuses.mutate(ordered.map((item, position) => ({ id: item.id, expected_version: item.version, position })))
                                  }
                                }
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
                              createStatus.mutate({ ...values, category })
                              setEditor(null)
                            }}
                          />
                        ) : null}
                      </div>
                    )
                  })}
                </div>
                {createStatus.isError ? <p role="alert" className="text-danger">Status creation failed. <button className="button button-ghost" onClick={() => createStatus.variables && createStatus.mutate(createStatus.variables)}>Retry</button></p> : null}
                {updateStatus.isError ? <p role="alert" className="text-danger">Status update failed. <button className="button button-ghost" onClick={() => updateStatus.variables && updateStatus.mutate(updateStatus.variables)}>Retry</button></p> : null}
                {deleteStatus.isError ? <p role="alert" className="text-danger">Status deletion failed. <button className="button button-ghost" onClick={() => deleteStatus.variables && deleteStatus.mutate(deleteStatus.variables)}>Retry</button></p> : null}
                {reorderStatuses.isError ? <p role="alert" className="text-danger">Status reorder failed. <button className="button button-ghost" onClick={() => reorderStatuses.variables && reorderStatuses.mutate(reorderStatuses.variables)}>Retry</button></p> : null}
                {createStatus.isPending || updateStatus.isPending || deleteStatus.isPending || reorderStatuses.isPending ? <p role="status">Saving statuses…</p> : null}
              </SettingsCard>

              <SettingsCard title="Danger zone" description="Deleting a project moves the project and all of its tasks to trash.">
                <button type="button" className="button button-danger" onClick={() => setConfirmDelete(true)}>
                  Delete project
                </button>
                {deleteProject.isError ? <p role="alert" className="text-danger">Project deletion failed. <button className="button button-ghost" onClick={() => deleteProject.variables && deleteProject.mutate(deleteProject.variables, { onSuccess: () => navigate('/tasks') })}>Retry</button></p> : null}
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
            deleteStatus.mutate({ statusId: deleteTarget.id, version: deleteTarget.version })
            setDeleteTarget(null)
          }}
        />
      ) : null}

      {confirmDelete && project ? (
        <ConfirmDeleteModal
          title="Delete project?"
          description={`This will move "${project.name}" and its ${tasks.length} ${tasks.length === 1 ? 'task' : 'tasks'} to trash.`}
          onClose={() => setConfirmDelete(false)}
          onConfirm={() => {
            deleteProject.mutate({ projectId: project.id, version: project.version }, { onSuccess: () => navigate('/tasks') })
            setConfirmDelete(false)
          }}
        />
      ) : null}
    </div>
  )
}

/**
 * General card with a draft: edits stay local until "Save Changes" (Reset drops them). The parent
 * keys this component by the saved values, so a fresh draft is created after each save.
 */
function ProjectGeneralCard({ project }: { project: Project }) {
  const { workspace } = useWorkspace()
  const updateProject = useUpdateProject(workspace.id, project.id)
  const [draft, setDraft] = useState({ name: project.name, key: project.key, color: project.color })
  const dirty = draft.name !== project.name || draft.key !== project.key || draft.color !== project.color
  const canSave = draft.name.trim().length > 0 && draft.key.length > 0

  return (
    <>
      <SettingsCard title="General" description="Name, tag and color of this project.">
        <div className="settings-grid">
          <div className="settings-field">
            <label className="field-label" htmlFor="project-name">
              Name
            </label>
            <input id="project-name" className="input" value={draft.name} onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))} />
          </div>
          <div className="settings-field">
            <label className="field-label" htmlFor="project-key">
              Tag
              <InfoTip text={`Short prefix used in task ids, for example ${project.key}-101.`} />
            </label>
            <input
              id="project-key"
              className="input"
              value={draft.key}
              maxLength={5}
              placeholder="INF"
              onChange={(e) => setDraft((d) => ({ ...d, key: e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, '') }))}
            />
          </div>
          <div className="settings-field col-span-2">
            <span className="field-label">Color</span>
            <div className="ps-color-palette ps-color-palette-inline">
              {PROJECT_COLORS.map((color) => (
                <button
                  key={color}
                  type="button"
                  className="ps-color-dot"
                  style={{ backgroundColor: color }}
                  title={color}
                  aria-label={`Color ${color}`}
                  aria-pressed={draft.color.toLowerCase() === color}
                  onClick={() => setDraft((d) => ({ ...d, color }))}
                >
                  {draft.color.toLowerCase() === color ? <TickCircle size={14} /> : null}
                </button>
              ))}
              <span className="ps-color-sep" />
              <label className="ps-color-dot ps-color-custom" title="Custom color">
                <input type="color" value={draft.color} aria-label="Custom color" onChange={(e) => setDraft((d) => ({ ...d, color: e.target.value }))} />
              </label>
            </div>
          </div>
        </div>
      </SettingsCard>
      {dirty ? (
        <UnsavedBar
          onReset={() => setDraft({ name: project.name, key: project.key, color: project.color })}
          onSave={() => canSave && updateProject.mutate({ name: draft.name.trim(), key: draft.key, color: draft.color, expected_version: project.version })}
          saving={!canSave || updateProject.isPending}
        />
      ) : null}
      {updateProject.isError ? <p role="alert" className="text-danger">Project update failed. <button className="button button-ghost" onClick={() => updateProject.variables && updateProject.mutate(updateProject.variables)}>Retry</button></p> : null}
    </>
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
