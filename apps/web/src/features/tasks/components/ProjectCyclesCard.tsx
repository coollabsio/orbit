import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import { ArrowRight, MoreH as MoreHorizontal, Edit as Pencil, Trash as Trash2 } from 'reicon-react'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Tip } from '@/components/common/Tip'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { LoadingText } from '@/components/ui/spinner'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import type { Cycle } from '@/features/tasks/api/models'
import { useCreateCycle, useCycles, useDeleteCycle, useMoveUnfinishedCycleTasks, useUpdateCycle } from '@/features/tasks/api/cycles'
import { changedCycleSpan, cycleDates, cycleDayRange, cycleName, cycleTiming, nextCycleSpan, planningProgress } from '@/features/tasks/planningLib'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** Only the dates the user picked anew: a day left as shown keeps its stored instant. */
type CycleValues = { name: string | null; starts_at?: string; ends_at?: string }

/** What went wrong, in words: overlapping dates and a missing next cycle are the usual answers. */
function cycleError(error: Error | null, fallback: string): string {
  if (error instanceof ApiProblem && error.code === 'cycle_overlap') return 'These dates overlap another cycle of the project.'
  if (error instanceof ApiProblem && error.code === 'validation_failed') return 'The last day must not be before the first day.'
  return fallback
}

function CycleForm({ initial, span, submitLabel, pending, onSubmit, onCancel }: { initial?: Cycle; span: Pick<Cycle, 'starts_at' | 'ends_at'>; submitLabel: string; pending: boolean; onSubmit: (values: CycleValues) => Promise<void>; onCancel?: () => void }) {
  const [name, setName] = useState(initial?.name ?? '')
  const [start, setStart] = useState(() => cycleDayRange(span).start)
  const [end, setEnd] = useState(() => cycleDayRange(span).end)
  const valid = start !== '' && end !== '' && end >= start
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!valid || pending) return
    try {
      await onSubmit({ name: name.trim() || null, ...changedCycleSpan(span, start, end) })
    } catch {
      // The card shows the mutation error.
    }
  }
  return (
    <form className="flex w-full flex-wrap items-center gap-3" onSubmit={(event) => void submit(event)} onKeyDown={(event) => { if (event.key === 'Escape') onCancel?.() }}>
      <Input className="w-full max-w-[200px]" aria-label="Cycle name" placeholder={initial ? `Cycle ${initial.number}` : 'Name (optional)'} maxLength={100} value={name} onChange={(event) => setName(event.target.value)} autoFocus={Boolean(initial)} />
      <Input className="w-auto" type="date" aria-label="First day" value={start} onChange={(event) => setStart(event.target.value)} />
      <span aria-hidden className="text-muted-foreground">–</span>
      <Input className="w-auto" type="date" aria-label="Last day" min={start} value={end} onChange={(event) => setEnd(event.target.value)} />
      <div className="flex gap-2">
        <Button type="submit" disabled={!valid || pending}>{submitLabel}</Button>
        {onCancel ? <Button type="button" variant="ghost" onClick={onCancel}>Cancel</Button> : null}
      </div>
    </form>
  )
}

/**
 * The project's cycles by date: create (the next one starts where the last ends and is as long), edit, delete, and
 * move a cycle's unfinished tasks to the next cycle. Current, next and previous come from today's date.
 */
export function ProjectCyclesCard({ projectId }: { projectId: string }) {
  const { workspace } = useWorkspace()
  const cycles = useCycles(workspace.id, projectId)
  const createCycle = useCreateCycle(workspace.id, projectId)
  const updateCycle = useUpdateCycle(workspace.id, projectId)
  const deleteCycle = useDeleteCycle(workspace.id, projectId)
  const moveUnfinished = useMoveUnfinishedCycleTasks(workspace.id, projectId)
  const [editingId, setEditingId] = useState<string>()
  const list = cycles.data ?? []
  const next = nextCycleSpan(list)
  const lastCycle = list.at(-1)

  const remove = async (cycle: Cycle) => {
    if (!await confirmAction({ title: `Delete ${cycleName(cycle)}?`, description: 'Its tasks stay and lose the cycle. You cannot undo this.', confirmLabel: 'Delete', danger: true })) return
    deleteCycle.mutate(cycle)
  }
  const moveTasks = (cycle: Cycle) => moveUnfinished.mutate(cycle.id, {
    onSuccess: ({ moved, to_cycle }) => toast(moved === 0 ? 'No unfinished tasks to move.' : `Moved ${moved} ${moved === 1 ? 'task' : 'tasks'} to ${cycleName(to_cycle)}.`),
  })
  const movedFrom = list.find((cycle) => cycle.id === moveUnfinished.variables)
  const moveError = moveUnfinished.error instanceof ApiProblem && moveUnfinished.error.code === 'no_next_cycle'
    ? `${movedFrom ? cycleName(movedFrom) : 'This cycle'} has no next cycle. Create the next cycle first, then move the tasks.`
    : 'The tasks could not be moved.'

  return (
    <SettingsCard title="Cycles" description="Time boxes for the project's work. A task can be in one cycle." flush>
      <div className="flex flex-col divide-y">
        <SettingsRow>
          {/* keyed by the last cycle, so the form suggests the following dates once one is created */}
          <CycleForm key={`${lastCycle?.id ?? 'none'}:${lastCycle?.ends_at ?? ''}`} span={next} submitLabel="Create cycle" pending={createCycle.isPending} onSubmit={async (values) => { await createCycle.mutateAsync({ ...next, ...values }) }} />
        </SettingsRow>
        {createCycle.isError ? <SettingsRow className="text-xs text-destructive" role="alert">{cycleError(createCycle.error, 'The cycle could not be created.')}</SettingsRow> : null}
        {cycles.isPending ? <SettingsRow><LoadingText>Loading cycles…</LoadingText></SettingsRow> : null}
        {cycles.isError ? <SettingsRow role="alert">Cycles could not be loaded.</SettingsRow> : null}
        {cycles.isSuccess && list.length === 0 ? <SettingsRow>No cycles yet.</SettingsRow> : null}
        {list.map((cycle) => {
          const timing = cycleTiming(cycle)
          const unfinished = cycle.task_count - cycle.completed_count
          return editingId === cycle.id
            ? <SettingsRow key={cycle.id}>
                <CycleForm
                  initial={cycle}
                  span={cycle}
                  submitLabel="Save"
                  pending={updateCycle.isPending}
                  onCancel={() => setEditingId(undefined)}
                  onSubmit={async (values) => {
                    await updateCycle.mutateAsync({ cycleId: cycle.id, body: { ...values, expected_version: cycle.version } })
                    setEditingId(undefined)
                  }}
                />
              </SettingsRow>
            : <SettingsRow key={cycle.id} className="group/row flex-nowrap" data-timing={timing}>
                <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="flex min-w-0 items-center gap-2 text-sm font-medium text-foreground in-data-[timing=past]:text-muted-foreground">
                    <span className="truncate">{cycleName(cycle)}</span>
                    {timing === 'current' ? <Badge>Current</Badge> : null}
                  </span>
                  <span className="text-xs text-muted-foreground/70 tabular-nums">{cycleDates(cycle)}</span>
                </div>
                <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{planningProgress(cycle)}</span>
                <DropdownMenu>
                  <Tip label="Actions">
                    <DropdownMenuTrigger
                      render={
                        <Button variant="ghost" size="icon-sm" className="size-[30px] rounded-full bg-muted transition-opacity duration-150 ease-out focus-visible:opacity-100 aria-expanded:opacity-100 hover-fine:opacity-0 hover-fine:group-hover/row:opacity-100" aria-label={`${cycleName(cycle)} actions`}>
                          <MoreHorizontal className="size-4" />
                        </Button>
                      }
                    />
                  </Tip>
                  <DropdownMenuContent align="end" className="w-auto min-w-45">
                    <DropdownMenuItem onClick={() => { updateCycle.reset(); setEditingId(cycle.id) }}>
                      <Pencil className="size-3.5" />
                      Edit
                    </DropdownMenuItem>
                    <DropdownMenuItem disabled={unfinished === 0 || moveUnfinished.isPending} onClick={() => moveTasks(cycle)}>
                      <ArrowRight className="size-3.5" />
                      Move unfinished tasks to next cycle
                    </DropdownMenuItem>
                    {/* data-danger (not variant="destructive"): the menu popup forces destructive items to the accent colour */}
                    <DropdownMenuItem className="text-destructive focus:bg-destructive/10 focus:text-destructive *:[svg]:text-destructive" data-danger="true" disabled={deleteCycle.isPending} onClick={() => void remove(cycle)}>
                      <Trash2 className="size-3.5" />
                      Delete
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </SettingsRow>
        })}
        {updateCycle.isError ? <SettingsRow className="text-xs text-destructive" role="alert">{cycleError(updateCycle.error, 'The cycle could not be saved. Someone else may have changed it; try again.')}</SettingsRow> : null}
        {moveUnfinished.isError ? <SettingsRow className="text-xs text-destructive" role="alert">{moveError}</SettingsRow> : null}
        {deleteCycle.isError ? <SettingsRow className="text-xs text-destructive" role="alert">The cycle could not be deleted.</SettingsRow> : null}
      </div>
    </SettingsCard>
  )
}
