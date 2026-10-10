import { useId, useState } from 'react'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useCycleSettings, useUpdateCycleSettings, type CycleSettings } from '@/features/tasks/api/cycles'
import type { Project, TaskStatusDef } from '@/features/tasks/api/models'
import { useUpdateProject } from '@/features/tasks/api/projects'
import { ESTIMATE_SCALES, SCALE_LABEL, WEEKDAYS, isEstimateScale } from '@/features/tasks/cyclesLib'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** The estimate scale of the project. A change of scale changes no task: the stored value is always points. */
export function EstimatesCard({ project }: { project: Project }) {
  const { workspace } = useWorkspace()
  const updateProject = useUpdateProject(workspace.id, project.id)
  const scale = isEstimateScale(project.estimate_scale) ? project.estimate_scale : null
  const save = (next: string | null) =>
    updateProject.mutate({ name: project.name, key: project.key, color: project.color, expected_version: project.version, estimate_scale: next })
  return (
    <SettingsCard title="Estimates" description="How large a task is, in points. A cycle shows its scope and progress in points.">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="text-sm font-medium text-foreground">Estimate scale</span>
          <span className="text-xs text-muted-foreground">With estimates off, tasks keep their values and do not show them.</span>
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="outline" size="sm" className="font-normal" disabled={updateProject.isPending} aria-label={`Estimate scale: ${scale ?? 'off'}`}>{scale ? SCALE_LABEL[scale] : 'Off'}</Button>} />
          <DropdownMenuContent align="end" className="w-auto min-w-56">
            <DropdownMenuItem className="data-selected:bg-accent data-selected:font-medium" data-selected={scale === null || undefined} onClick={() => save(null)}>Off</DropdownMenuItem>
            {ESTIMATE_SCALES.map((value) => (
              <DropdownMenuItem key={value} className="data-selected:bg-accent data-selected:font-medium" data-selected={value === scale || undefined} onClick={() => save(value)}>{SCALE_LABEL[value]}</DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {updateProject.isError ? <p role="alert" className="mt-2 text-xs text-destructive">Couldn’t save the estimate scale. Try again.</p> : null}
    </SettingsCard>
  )
}

const NO_CYCLE_LABEL: Record<string, string> = {
  off: 'Leave them',
  backlog: 'Move them to the backlog',
  cycle: 'Add them to the current cycle',
}

/** The timezone of this browser; the default for a project that turns cycles on. */
function browserTimezone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
}

/** The cycle settings of the project. A change applies to future cycles that have no tasks. */
export function CyclesCard({ project, statuses }: { project: Project; statuses: TaskStatusDef[] }) {
  const { workspace } = useWorkspace()
  const settings = useCycleSettings(workspace.id, project.id).data
  if (!settings) return null
  return <CyclesForm key={settings.version} project={project} settings={settings} hasBacklog={statuses.some((status) => status.category === 'backlog')} />
}

function NumberChoice({ label, value, values, unit, disabled, onChange }: { label: string; value: number; values: number[]; unit: (count: number) => string; disabled: boolean; onChange: (value: number) => void }) {
  return (
    <Field className="w-auto">
      <FieldLabel>{label}</FieldLabel>
      <DropdownMenu>
        <DropdownMenuTrigger render={<Button type="button" variant="outline" size="sm" className="font-normal" disabled={disabled} aria-label={`${label}: ${unit(value)}`}>{unit(value)}</Button>} />
        <DropdownMenuContent className="max-h-64 w-auto min-w-32">
          {values.map((option) => (
            <DropdownMenuItem key={option} className="data-selected:bg-accent data-selected:font-medium" data-selected={option === value || undefined} onClick={() => onChange(option)}>{unit(option)}</DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </Field>
  )
}

const range = (from: number, to: number) => Array.from({ length: to - from + 1 }, (_, index) => from + index)
const weeksText = (count: number) => `${count} ${count === 1 ? 'week' : 'weeks'}`

function CyclesForm({ project, settings, hasBacklog }: { project: Project; settings: CycleSettings; hasBacklog: boolean }) {
  const { workspace } = useWorkspace()
  const update = useUpdateCycleSettings(workspace.id, project.id)
  // a project that never used cycles starts in the timezone of the person who turns them on
  const [draft, setDraft] = useState(() => ({ ...settings, timezone: settings.version === 0 ? browserTimezone() : settings.timezone }))
  const timezoneId = useId()
  const set = (fields: Partial<CycleSettings>) => setDraft((current) => ({ ...current, ...fields }))
  const busy = update.isPending
  const changed = (Object.keys(draft) as Array<keyof CycleSettings>).some((key) => draft[key] !== settings[key]) && (draft.enabled || settings.enabled)
  const save = (next: typeof draft) => {
    const { project_id: _project, version, ...body } = next
    update.mutate({ ...body, expected_version: version })
  }
  return (
    <SettingsCard title="Cycles" description="Repeating work periods. Orbit creates each cycle and moves open work to the next one when a cycle ends.">
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="text-sm font-medium text-foreground">Use cycles</span>
          <span className="text-xs text-muted-foreground">With cycles off, the current cycle is completed and the future cycles are deleted. Completed cycles stay.</span>
        </div>
        <Switch
          aria-label="Use cycles"
          checked={draft.enabled}
          disabled={busy}
          onCheckedChange={(enabled: boolean) => {
            const next = { ...draft, enabled }
            setDraft(next)
            // the switch saves at once; the other fields save with the button
            save(next)
          }}
        />
      </div>
      {draft.enabled ? (
        <div className="mt-4 flex flex-col gap-4 border-t pt-4">
          <div className="flex flex-wrap items-end gap-3">
            <NumberChoice label="Length" value={draft.weeks} values={range(1, 8)} unit={weeksText} disabled={busy} onChange={(weeks) => set({ weeks })} />
            <NumberChoice label="Starts on" value={draft.start_weekday} values={range(0, 6)} unit={(day) => WEEKDAYS[day] ?? ''} disabled={busy} onChange={(start_weekday) => set({ start_weekday })} />
            <NumberChoice label="Cooldown" value={draft.cooldown_weeks} values={range(0, 4)} unit={(count) => (count === 0 ? 'None' : weeksText(count))} disabled={busy} onChange={(cooldown_weeks) => set({ cooldown_weeks })} />
            <NumberChoice label="Cycles ahead" value={draft.cycles_ahead} values={range(1, 15)} unit={String} disabled={busy} onChange={(cycles_ahead) => set({ cycles_ahead })} />
            <Field className="w-auto">
              <FieldLabel htmlFor={timezoneId}>Timezone</FieldLabel>
              <Input id={timezoneId} className="h-8 w-52 text-[13px]" placeholder="Europe/Berlin" value={draft.timezone} disabled={busy} onChange={(event) => set({ timezone: event.target.value.trim() })} />
            </Field>
          </div>
          <div className="flex flex-col divide-y divide-border">
            <label className="flex items-start justify-between gap-4 py-3 first:pt-0">
              <span className="flex min-w-0 flex-col gap-0.5">
                <span className="text-sm font-medium text-foreground">Add started tasks to the current cycle</span>
                <span className="text-xs text-muted-foreground">A task with no cycle that moves to a started status. During a cooldown: the next cycle.</span>
              </span>
              <Switch aria-label="Add started tasks to the current cycle" checked={draft.auto_add_started} disabled={busy} onCheckedChange={(checked: boolean) => set({ auto_add_started: checked })} />
            </label>
            <label className="flex items-start justify-between gap-4 py-3">
              <span className="flex min-w-0 flex-col gap-0.5">
                <span className="text-sm font-medium text-foreground">Add completed tasks to the current cycle</span>
                <span className="text-xs text-muted-foreground">A task with no cycle that moves to a completed status. During a cooldown: the cycle before.</span>
              </span>
              <Switch aria-label="Add completed tasks to the current cycle" checked={draft.auto_add_completed} disabled={busy} onCheckedChange={(checked: boolean) => set({ auto_add_completed: checked })} />
            </label>
            <div className="flex items-start justify-between gap-4 py-3 last:pb-0">
              <span className="flex min-w-0 flex-col gap-0.5">
                <span className="text-sm font-medium text-foreground">Active tasks with no cycle</span>
                <span className="text-xs text-muted-foreground">What happens to a task in an unstarted status that is in no cycle.</span>
              </span>
              <DropdownMenu>
                <DropdownMenuTrigger render={<Button type="button" variant="outline" size="sm" className="font-normal" disabled={busy}>{NO_CYCLE_LABEL[draft.active_without_cycle] ?? draft.active_without_cycle}</Button>} />
                <DropdownMenuContent align="end" className="w-auto min-w-56">
                  {Object.keys(NO_CYCLE_LABEL).map((value) => (
                    <DropdownMenuItem
                      key={value}
                      className="data-selected:bg-accent data-selected:font-medium"
                      data-selected={value === draft.active_without_cycle || undefined}
                      // the backlog option needs a backlog status in the project
                      disabled={value === 'backlog' && !hasBacklog}
                      onClick={() => set({ active_without_cycle: value })}
                    >
                      {NO_CYCLE_LABEL[value]}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
          <div className="flex items-center justify-end gap-3">
            <span className="mr-auto text-xs text-muted-foreground">A change applies to future cycles that have no tasks. Cycles that have tasks keep their dates.</span>
            <Button size="sm" disabled={!changed || busy || !draft.timezone} onClick={() => save(draft)}>{busy ? 'Saving…' : 'Save cycles'}</Button>
          </div>
        </div>
      ) : null}
      {update.isError ? <p role="alert" className="mt-2 text-xs text-destructive">Couldn’t save the cycle settings. Check the timezone name (for example Europe/Berlin) and try again.</p> : null}
    </SettingsCard>
  )
}
