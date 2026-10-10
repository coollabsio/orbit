import { useState } from 'react'
import { Edit as Pencil, MoreH as MoreHorizontal, Add as Plus } from 'reicon-react'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Switch } from '@/components/ui/switch'
import { useRecurringTaskMutations, useRecurringTasks, useTemplateMutations, useTemplates, type RecurringTask, type Template } from '@/features/tasks/api/intake'
import { useLabels } from '@/features/tasks/api/labels'
import { useMilestones } from '@/features/tasks/api/milestones'
import type { Project, TaskStatusDef } from '@/features/tasks/api/models'
import { useUpdateProject } from '@/features/tasks/api/projects'
import { intervalText, nextRunText } from '@/features/tasks/recurringLib'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { RecurringTaskModal, TemplateModal, type PayloadOptions } from './PayloadEditor'

/** The number of tasks in the queue, from a 409 `triage_not_empty` answer; null for any other error. */
function triageCount(error: unknown): number | null {
  const problem = error as { code?: string; conflict?: { count?: number } } | null
  return problem?.code === 'triage_not_empty' ? problem.conflict?.count ?? 0 : null
}

/** Triage is a setting of the project. It cannot go off while tasks wait in the queue. */
export function TriageCard({ project }: { project: Project }) {
  const { workspace } = useWorkspace()
  const updateProject = useUpdateProject(workspace.id, project.id)
  const waiting = triageCount(updateProject.error)
  return (
    <SettingsCard title="Triage" description="Review work that comes from outside the team before it enters the workflow.">
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="text-sm font-medium text-foreground">Send new tasks from integrations to triage</span>
          <span className="text-xs text-muted-foreground">Tasks from GitHub and Discord wait in the Triage queue until a member accepts or declines them.</span>
        </div>
        <Switch
          aria-label="Triage"
          checked={project.triage_enabled}
          disabled={updateProject.isPending}
          onCheckedChange={(checked: boolean) => updateProject.mutate({ name: project.name, key: project.key, color: project.color, expected_version: project.version, triage_enabled: checked })}
        />
      </div>
      {updateProject.isError ? (
        <p role="alert" className="mt-2 text-xs text-destructive">
          {waiting === null
            ? 'Couldn’t save the triage setting. Try again.'
            : `Triage can’t be turned off while ${waiting} ${waiting === 1 ? 'task waits' : 'tasks wait'} in the queue. Accept or decline ${waiting === 1 ? 'it' : 'them'} first.`}
        </p>
      ) : null}
    </SettingsCard>
  )
}

function usePayloadOptions(project: Project, statuses: TaskStatusDef[]): PayloadOptions {
  const { workspace } = useWorkspace()
  const members = useMembers(workspace.id).data ?? []
  const labels = useLabels(workspace.id).data ?? []
  const milestones = (useMilestones(workspace.id).data ?? []).filter((milestone) => milestone.project_id === project.id)
  return {
    statuses: statuses.filter((status) => status.category !== 'duplicate' && (status.category !== 'triage' || project.triage_enabled)),
    milestones,
    members,
    labels,
  }
}

type Editing<T> = { mode: 'new' } | { mode: 'edit'; item: T } | null

/** The task templates of the project. */
export function TemplatesCard({ project, statuses }: { project: Project; statuses: TaskStatusDef[] }) {
  const { workspace } = useWorkspace()
  const templates = useTemplates(workspace.id, project.id).data ?? []
  const { create, update, remove } = useTemplateMutations(workspace.id, project.id)
  const options = usePayloadOptions(project, statuses)
  const [editing, setEditing] = useState<Editing<Template>>(null)
  const close = () => {
    create.reset()
    update.reset()
    setEditing(null)
  }
  const confirmDelete = async (template: Template) => {
    if (await confirmAction({ title: `Delete “${template.name}”?`, description: 'Tasks that were made from this template do not change.', confirmLabel: 'Delete template', danger: true })) remove.mutate(template)
  }
  return (
    <SettingsCard
      title="Templates"
      description="Start a common task from a template: its fields and its sub-issues."
      actions={<Button variant="outline" size="sm" onClick={() => setEditing({ mode: 'new' })}><Plus aria-hidden />New template</Button>}
    >
      {templates.length === 0 ? (
        <p className="text-sm text-muted-foreground">This project has no templates.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border">
          {templates.map((template) => (
            <li key={template.id} className="flex items-center gap-3 py-2 first:pt-0 last:pb-0">
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-sm font-medium text-foreground">{template.name}</span>
                <span className="truncate text-xs text-muted-foreground">
                  {template.payload.title}
                  {(template.payload.sub_issues?.length ?? 0) > 0 ? ` · ${template.payload.sub_issues!.length} sub-issue${template.payload.sub_issues!.length === 1 ? '' : 's'}` : ''}
                </span>
              </div>
              <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label={`Edit ${template.name}`} onClick={() => setEditing({ mode: 'edit', item: template })}><Pencil /></Button>
              <DropdownMenu>
                <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label={`${template.name} actions`}><MoreHorizontal /></Button>} />
                <DropdownMenuContent align="end" className="w-auto min-w-36">
                  <DropdownMenuItem variant="destructive" onClick={() => void confirmDelete(template)}>Delete…</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </li>
          ))}
        </ul>
      )}
      {remove.isError ? <p role="alert" className="mt-2 text-xs text-destructive">The template was not deleted. Try again.</p> : null}
      {editing ? (
        <TemplateModal
          template={editing.mode === 'edit' ? editing.item : undefined}
          options={options}
          busy={create.isPending || update.isPending}
          failed={create.isError || update.isError}
          onClose={close}
          onSave={(name, payload) => {
            if (editing.mode === 'edit') update.mutate({ templateId: editing.item.id, body: { expected_version: editing.item.version, name, payload } }, { onSuccess: close })
            else create.mutate({ name, payload }, { onSuccess: close })
          }}
        />
      ) : null}
    </SettingsCard>
  )
}

/** The recurring tasks of the project: routine work that creates its own tasks. */
export function RecurringTasksCard({ project, statuses }: { project: Project; statuses: TaskStatusDef[] }) {
  const { workspace } = useWorkspace()
  const routines = useRecurringTasks(workspace.id, project.id).data ?? []
  const { create, update, remove } = useRecurringTaskMutations(workspace.id, project.id)
  const options = usePayloadOptions(project, statuses)
  const [editing, setEditing] = useState<Editing<RecurringTask>>(null)
  const close = () => {
    create.reset()
    update.reset()
    setEditing(null)
  }
  const confirmDelete = async (routine: RecurringTask) => {
    if (await confirmAction({ title: `Delete “${routine.payload.title}”?`, description: 'No more tasks are created. The tasks that exist do not change.', confirmLabel: 'Delete recurring task', danger: true })) remove.mutate(routine)
  }
  return (
    <SettingsCard
      title="Recurring tasks"
      description="Routine work that creates its own tasks, on a schedule or after the task before it is closed."
      actions={<Button variant="outline" size="sm" onClick={() => setEditing({ mode: 'new' })}><Plus aria-hidden />New recurring task</Button>}
    >
      {routines.length === 0 ? (
        <p className="text-sm text-muted-foreground">This project has no recurring tasks.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border">
          {routines.map((routine) => (
            <li key={routine.id} className="flex items-center gap-3 py-2 first:pt-0 last:pb-0">
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-sm font-medium text-foreground">{routine.payload.title}</span>
                <span className="truncate text-xs text-muted-foreground">{intervalText(routine.every_count, routine.every_unit, routine.mode)} · Next: {nextRunText(routine)}</span>
              </div>
              <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label={`Edit ${routine.payload.title}`} onClick={() => setEditing({ mode: 'edit', item: routine })}><Pencil /></Button>
              <DropdownMenu>
                <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label={`${routine.payload.title} actions`}><MoreHorizontal /></Button>} />
                <DropdownMenuContent align="end" className="w-auto min-w-36">
                  <DropdownMenuItem onClick={() => update.mutate({ recurringId: routine.id, body: { expected_version: routine.version, paused: !routine.paused } })}>{routine.paused ? 'Resume' : 'Pause'}</DropdownMenuItem>
                  <DropdownMenuItem variant="destructive" onClick={() => void confirmDelete(routine)}>Delete…</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </li>
          ))}
        </ul>
      )}
      {remove.isError || (update.isError && !editing) ? <p role="alert" className="mt-2 text-xs text-destructive">The change was not saved. Try again.</p> : null}
      {editing ? (
        <RecurringTaskModal
          routine={editing.mode === 'edit' ? editing.item : undefined}
          options={options}
          busy={create.isPending || update.isPending}
          failed={create.isError || update.isError}
          onClose={close}
          onSave={(values) => {
            if (editing.mode === 'edit') update.mutate({ recurringId: editing.item.id, body: { expected_version: editing.item.version, ...values } }, { onSuccess: close })
            else create.mutate(values, { onSuccess: close })
          }}
        />
      ) : null}
    </SettingsCard>
  )
}
