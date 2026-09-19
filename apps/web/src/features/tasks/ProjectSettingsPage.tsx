import { useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { ArrowLeft, Check, MoreHorizontal, Pencil, Plus, SquareCheck, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
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

const PROJECT_COLORS = [
  '#8b5cf6', '#6366f1', '#0ea5e9', '#06b6d4', '#10b981', '#22c55e', '#84cc16', '#eab308', '#f59e0b', '#f97316', '#ef4444',
  '#ec4899', '#d946ef', '#64748b', '#78716c',
]

const FIELD_LABEL = 'mb-1.5 flex h-4 items-center gap-1 text-[13px] leading-4 font-medium text-muted-foreground'
const OPTION =
  'group flex w-full min-h-8 items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-accent disabled:pointer-events-none disabled:opacity-50 data-[tone=danger]:text-destructive'
const COLOR_DOT =
  'relative inline-flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-full text-white aria-pressed:ring-2 aria-pressed:ring-foreground/60 aria-pressed:ring-offset-2 aria-pressed:ring-offset-background'
const TILE = 'inline-flex size-10 items-center justify-center rounded-lg bg-muted'

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
    return <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background"><EmptyState icon={SquareCheck} title="Loading project" description="Loading persisted project settings." /></section></div>
  }
  if (projectsQuery.isError || statusQuery.isError || tasksQuery.isError) {
    return <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background"><EmptyState icon={SquareCheck} title="Project unavailable" description="The server could not load this project." /></section></div>
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <section className="relative flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
        <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border px-3 py-2 max-[899px]:border-b-0">
          <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={() => navigate('/tasks')} aria-label="Back to tasks">
            <ArrowLeft className="size-4" />
          </Button>
          <span className="truncate text-[13px] font-semibold text-foreground">{project ? `${project.name} settings` : 'Project settings'}</span>
        </div>
        {!project ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <EmptyState icon={SquareCheck} title="Project not found" description="This project does not exist or was removed." />
          </div>
        ) : (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <div className="mx-auto flex w-full max-w-[800px] flex-col gap-6 px-10 pt-7 pb-12 max-[899px]:px-5">
              <ProjectGeneralCard key={`${project.id}:${project.name}:${project.key}:${project.color}`} project={project} />

              <SettingsCard title="Statuses" description="The workflow a task goes through from start to completion." flush>
                <div className="flex flex-col py-3 pr-3 pl-[18px]">
                  {CATEGORY_ORDER.map((category) => {
                    const own = statuses.filter((s) => s.category === category)
                    return (
                      <div key={category}>
                        <div className="flex items-center justify-between rounded-md bg-muted px-3 py-2 text-[13px] text-muted-foreground">
                          <span>{CATEGORY_LABEL[category]}</span>
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            className="size-6 text-muted-foreground/70"
                            aria-label={`Add ${CATEGORY_LABEL[category].toLowerCase()} status`}
                            title="Add status"
                            onClick={() => setEditor({ mode: 'new', category })}
                          >
                            <Plus className="size-3.5" />
                          </Button>
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
                              className="group/row relative flex items-center gap-3 px-1 py-2.5 data-[dragging]:rounded-lg data-[dragging]:bg-muted data-[drop=before]:before:absolute data-[drop=before]:before:inset-x-0 data-[drop=before]:before:-top-px data-[drop=before]:before:h-0.5 data-[drop=before]:before:rounded-[1px] data-[drop=before]:before:bg-primary data-[drop=before]:before:content-[''] data-[drop=after]:after:absolute data-[drop=after]:after:inset-x-0 data-[drop=after]:after:-bottom-px data-[drop=after]:after:h-0.5 data-[drop=after]:after:rounded-[1px] data-[drop=after]:after:bg-primary data-[drop=after]:after:content-['']"
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
                              <span className="absolute -left-[13px] inline-flex text-muted-foreground/70 opacity-0 transition-opacity group-hover/row:opacity-100" aria-hidden="true">
                                <GripIcon />
                              </span>
                              <span className={TILE}>
                                <TaskStatusIcon status={status} size={16} />
                              </span>
                              <span className="flex flex-col gap-0.5">
                                <span className="text-sm font-medium text-foreground">
                                  {status.name}
                                  {status.id === defaultStatus?.id ? <span className="font-normal text-muted-foreground"> · Default</span> : null}
                                </span>
                                {status.description ? (
                                  <span className="text-xs text-muted-foreground/70">{status.description}</span>
                                ) : countFor(status.id) > 0 ? (
                                  <span className="text-xs text-muted-foreground/70">
                                    {countFor(status.id)} {countFor(status.id) === 1 ? 'task' : 'tasks'}
                                  </span>
                                ) : null}
                              </span>
                              <div className="flex-1" />
                              <Dropdown
                                align="right"
                                trigger={() => (
                                  <Button variant="ghost" size="icon-sm" className="size-[30px] rounded-full bg-muted opacity-0 transition-opacity group-hover/row:opacity-100 focus-visible:opacity-100 aria-expanded:opacity-100" aria-label={`${status.name} actions`} title="Actions">
                                    <MoreHorizontal className="size-4" />
                                  </Button>
                                )}
                              >
                                {(close) => (
                                  <div className="flex min-w-[180px] flex-col gap-px p-1">
                                    <button
                                      className={OPTION}
                                      onClick={() => {
                                        setEditor({ mode: 'edit', statusId: status.id })
                                        close()
                                      }}
                                    >
                                      <Pencil className="size-3.5" />
                                      Edit
                                    </button>
                                    <button
                                      className={OPTION}
                                      data-tone="danger"
                                      disabled={statuses.length === 1}
                                      onClick={() => {
                                        setDeleteTarget(status)
                                        close()
                                      }}
                                    >
                                      <Trash2 className="size-3.5" />
                                      Delete
                                    </button>
                                  </div>
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
                {createStatus.isError ? <p role="alert" className="text-destructive">Status creation failed. <Button variant="ghost" onClick={() => createStatus.variables && createStatus.mutate(createStatus.variables)}>Retry</Button></p> : null}
                {updateStatus.isError ? <p role="alert" className="text-destructive">Status update failed. <Button variant="ghost" onClick={() => updateStatus.variables && updateStatus.mutate(updateStatus.variables)}>Retry</Button></p> : null}
                {deleteStatus.isError ? <p role="alert" className="text-destructive">Status deletion failed. <Button variant="ghost" onClick={() => deleteStatus.variables && deleteStatus.mutate(deleteStatus.variables)}>Retry</Button></p> : null}
                {reorderStatuses.isError ? <p role="alert" className="text-destructive">Status reorder failed. <Button variant="ghost" onClick={() => reorderStatuses.variables && reorderStatuses.mutate(reorderStatuses.variables)}>Retry</Button></p> : null}
                {createStatus.isPending || updateStatus.isPending || deleteStatus.isPending || reorderStatuses.isPending ? <p role="status">Saving statuses…</p> : null}
              </SettingsCard>

              <SettingsCard title="Danger zone" description="Deleting a project moves the project and all of its tasks to trash.">
                <Button variant="destructive" onClick={() => setConfirmDelete(true)}>
                  Delete project
                </Button>
                {deleteProject.isError ? <p role="alert" className="text-destructive">Project deletion failed. <Button variant="ghost" onClick={() => deleteProject.variables && deleteProject.mutate(deleteProject.variables, { onSuccess: () => navigate('/tasks') })}>Retry</Button></p> : null}
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
        <div className="grid grid-cols-2 gap-4 max-[899px]:grid-cols-1">
          <div className="w-full min-w-0">
            <Label className={FIELD_LABEL} htmlFor="project-name">
              Name
            </Label>
            <Input id="project-name" value={draft.name} onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))} />
          </div>
          <div className="w-full min-w-0">
            <Label className={FIELD_LABEL} htmlFor="project-key">
              Tag
              <InfoTip text={`Short prefix used in task ids, for example ${project.key}-101.`} />
            </Label>
            <Input
              id="project-key"
              value={draft.key}
              maxLength={5}
              placeholder="INF"
              onChange={(e) => setDraft((d) => ({ ...d, key: e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, '') }))}
            />
          </div>
          <div className="col-span-2 w-full min-w-0 max-[899px]:col-span-1">
            <span className={FIELD_LABEL}>Color</span>
            <div className="flex flex-wrap items-center gap-2 pt-1.5">
              {PROJECT_COLORS.map((color) => (
                <button
                  key={color}
                  type="button"
                  className={COLOR_DOT}
                  style={{ backgroundColor: color }}
                  title={color}
                  aria-label={`Color ${color}`}
                  aria-pressed={draft.color.toLowerCase() === color}
                  onClick={() => setDraft((d) => ({ ...d, color }))}
                >
                  {draft.color.toLowerCase() === color ? <Check className="size-3.5" /> : null}
                </button>
              ))}
              <span className="h-5 w-px bg-border" />
              <label className={COLOR_DOT} style={{ backgroundImage: 'conic-gradient(#eb5757, #f2c94c, #4cb782, #26b5ce, #5e6ad2, #a78bfa, #eb5757)' }} title="Custom color">
                <input type="color" value={draft.color} aria-label="Custom color" className="absolute inset-0 cursor-pointer opacity-0" onChange={(e) => setDraft((d) => ({ ...d, color: e.target.value }))} />
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
      {updateProject.isError ? <p role="alert" className="text-destructive">Project update failed. <Button variant="ghost" onClick={() => updateProject.variables && updateProject.mutate(updateProject.variables)}>Retry</Button></p> : null}
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
      className="flex items-center gap-2.5 px-1 py-2.5"
      onSubmit={(e) => {
        e.preventDefault()
        if (canSave) onSave({ name: name.trim(), description: description.trim(), color })
      }}
    >
      <Dropdown
        trigger={() => (
          <button type="button" className={`${TILE} cursor-pointer hover:bg-border`} aria-label="Status color" title="Color">
            <TaskStatusIcon status={{ category, color }} size={16} />
          </button>
        )}
      >
        {() => (
          <div className="flex items-center gap-2 p-1.5">
            {STATUS_COLORS.map((preset) => (
              <button
                key={preset}
                type="button"
                className={COLOR_DOT}
                style={{ backgroundColor: preset }}
                aria-label={`Color ${preset}`}
                aria-pressed={color === preset}
                onClick={() => setColor(preset)}
              >
                {color === preset ? <Check className="size-3.5" /> : null}
              </button>
            ))}
            <span className="h-5 w-px bg-border" />
            <label className={COLOR_DOT} style={{ backgroundImage: 'conic-gradient(#eb5757, #f2c94c, #4cb782, #26b5ce, #5e6ad2, #a78bfa, #eb5757)' }} title="Custom color">
              <input type="color" value={color} aria-label="Custom color" className="absolute inset-0 cursor-pointer opacity-0" onChange={(e) => setColor(e.target.value)} />
            </label>
          </div>
        )}
      </Dropdown>
      <Input
        className="h-9 w-[200px]"
        placeholder="Status name"
        aria-label="Status name"
        value={name}
        autoFocus
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') onCancel()
        }}
      />
      <Input
        className="h-9 min-w-0 flex-1"
        placeholder="Description…"
        aria-label="Description"
        value={description}
        onChange={(e) => setDescription(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') onCancel()
        }}
      />
      <Button type="button" variant="ghost" onClick={onCancel}>
        Cancel
      </Button>
      <Button type="submit" disabled={!canSave}>
        Save
      </Button>
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
