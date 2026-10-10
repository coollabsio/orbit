import type { Cycle } from '@/features/tasks/api/cycles'
import { useCycleDays } from '@/features/tasks/api/cycles'
import { useMilestoneBurnup, useOpenBy, useThroughput, type OpenDimension } from '@/features/tasks/api/insights'
import { useLabels } from '@/features/tasks/api/labels'
import type { Milestone } from '@/features/tasks/api/milestones'
import type { Project } from '@/features/tasks/api/models'
import { useProjectStatuses } from '@/features/tasks/api/projects'
import { cycleName } from '@/features/tasks/cyclesLib'
import { PRIORITY_LABEL } from '@/features/tasks/taskMeta'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { BarChart, BarList, ChartCard, ChartEmpty, LineChart, MeasureToggle, SERIES } from './Charts'
import { addDays, average, dayLabel, fillDays, idealLine, measureText, useMeasure, type Measure } from './chartLib'

/** "3 tasks have no estimate; they count 0 points." Only in points mode, and only when there are some. */
function unestimatedNote(count: number, measure: Measure): string | undefined {
  if (measure !== 'points' || count === 0) return undefined
  return `${count} ${count === 1 ? 'task has' : 'tasks have'} no estimate and ${count === 1 ? 'counts' : 'count'} 0 points.`
}

/** The measure toggle of a chart; nothing where the project has estimates off. */
function useChartMeasure(project: Project) {
  const [measure, setMeasure] = useMeasure(Boolean(project.estimate_scale))
  const toggle = project.estimate_scale ? <MeasureToggle measure={measure} onChange={setMeasure} /> : undefined
  return { measure, toggle, format: (value: number) => measureText(value, measure) }
}

/** Scope, started and done for each day of a cycle, with the ideal line from the scope of day 1 to zero at the end. */
export function BurndownChart({ cycle, project }: { cycle: Cycle; project: Project }) {
  const { workspace } = useWorkspace()
  const { measure, toggle, format } = useChartMeasure(project)
  const days = fillDays(useCycleDays(workspace.id, cycle).data ?? [])
  const pick = (kind: 'scope' | 'started' | 'done') => days.map((day) => day[`${kind}_${measure}`])
  // the ideal line covers the full cycle, also the days that are still to come
  const first = days[0]
  const total = first ? Math.max(days.length, Math.round((new Date(cycle.ends_at).getTime() - new Date(cycle.starts_at).getTime()) / 86_400_000)) : 0
  const labels = first ? Array.from({ length: total }, (_, index) => addDays(first.day, index)) : []
  const scope = pick('scope')
  const done = pick('done')
  const series = [
    { name: 'Scope', color: SERIES.scope, values: scope },
    { name: 'Started', color: SERIES.started, values: pick('started') },
    { name: 'Done', color: SERIES.done, values: done },
    { name: 'Ideal (remaining)', color: SERIES.neutral, values: idealLine(scope[0] ?? 0, total), dashed: true },
  ]
  const last = days.at(-1)
  return (
    <ChartCard
      title="Burndown"
      legend={series}
      actions={toggle}
      summary={last ? `${cycleName(cycle)}: ${format(last[`done_${measure}`])} done of ${format(last[`scope_${measure}`])} on ${dayLabel(last.day)}.` : `${cycleName(cycle)} has no data.`}
      note={unestimatedNote(cycle.unestimated_count, measure)}
    >
      {days.length < 2 ? <ChartEmpty>The burndown shows after the second day of the cycle.</ChartEmpty> : <LineChart series={series} labels={labels.map(dayLabel)} format={format} />}
    </ChartCard>
  )
}

/** What the project completed in each of its last 10 completed cycles, with the average. */
export function VelocityChart({ cycles, project }: { cycles: Cycle[]; project: Project }) {
  const { measure, toggle, format } = useChartMeasure(project)
  const completed = cycles.filter((cycle) => cycle.state === 'completed').slice(-10)
  const values = completed.map((cycle) => (measure === 'points' ? cycle.done_points : cycle.done_count))
  const mean = average(values)
  return (
    <ChartCard
      title="Velocity"
      actions={toggle}
      summary={completed.length > 0 ? `Done in the last ${completed.length} completed cycles: ${values.join(', ')}. Average ${format(mean)}.` : 'No completed cycle.'}
      note={unestimatedNote(completed.reduce((sum, cycle) => sum + cycle.unestimated_count, 0), measure)}
    >
      {completed.length === 0 ? (
        <ChartEmpty>The velocity shows after the first cycle is completed.</ChartEmpty>
      ) : (
        <BarChart
          series={[{ name: 'Done', color: SERIES.done, values }]}
          labels={completed.map((cycle) => `#${cycle.number}`)}
          tooltipLabels={completed.map(cycleName)}
          reference={{ value: mean, label: `Average ${mean}` }}
          format={format}
        />
      )}
    </ChartCard>
  )
}

/** Scope and done of a milestone for each week, with the target date as a vertical line. */
export function BurnupChart({ milestone, project }: { milestone: Milestone; project: Project }) {
  const { workspace } = useWorkspace()
  const { measure, toggle, format } = useChartMeasure(project)
  const data = useMilestoneBurnup(workspace.id, project.id, milestone.id).data
  const weeks = data?.items ?? []
  const series = [
    { name: 'Scope', color: SERIES.scope, values: weeks.map((week) => week[`scope_${measure}`]) },
    { name: 'Done', color: SERIES.done, values: weeks.map((week) => week[`done_${measure}`]) },
  ]
  // the week that holds the target date, when the chart reaches it
  const target = milestone.target_at ? weeks.findLastIndex((week) => week.week <= milestone.target_at!.slice(0, 10)) : -1
  const targetInRange = target >= 0 && milestone.target_at!.slice(0, 10) < addDays(weeks[target]!.week, 7)
  const last = weeks.at(-1)
  return (
    <ChartCard
      title="Burn-up"
      legend={series}
      actions={toggle}
      summary={last ? `${milestone.name}: ${format(last[`done_${measure}`])} done of ${format(last[`scope_${measure}`])} in the week of ${dayLabel(last.week)}.` : `${milestone.name} has no tasks.`}
      note={unestimatedNote(data?.unestimated_count ?? 0, measure)}
    >
      {weeks.length === 0 ? (
        <ChartEmpty>The burn-up shows when the milestone has tasks.</ChartEmpty>
      ) : (
        <LineChart series={series} labels={weeks.map((week) => dayLabel(week.week))} tooltipLabels={weeks.map((week) => `Week of ${dayLabel(week.week)}`)} marker={targetInRange ? { index: target, label: 'Target' } : undefined} format={format} />
      )}
    </ChartCard>
  )
}

/** Tasks created and tasks completed in each of the last 12 weeks. */
export function ThroughputChart({ project }: { project: Project }) {
  const { workspace } = useWorkspace()
  const { measure, toggle, format } = useChartMeasure(project)
  const data = useThroughput(workspace.id, project.id).data
  const weeks = data?.items ?? []
  const series = [
    { name: 'Created', color: SERIES.started, values: weeks.map((week) => week[`created_${measure}`]) },
    { name: 'Completed', color: SERIES.done, values: weeks.map((week) => week[`completed_${measure}`]) },
  ]
  const sum = (values: number[]) => values.reduce((total, value) => total + value, 0)
  return (
    <ChartCard
      title="Created and completed"
      legend={series}
      actions={toggle}
      summary={`In the last ${weeks.length} weeks: ${format(sum(series[0]!.values))} created, ${format(sum(series[1]!.values))} completed.`}
      note={unestimatedNote(data?.unestimated_count ?? 0, measure)}
    >
      {weeks.length === 0 ? <ChartEmpty>No data.</ChartEmpty> : <BarChart series={series} labels={weeks.map((week) => dayLabel(week.week))} tooltipLabels={weeks.map((week) => `Week of ${dayLabel(week.week)}`)} format={format} />}
    </ChartCard>
  )
}

const DIMENSION_TITLE: Record<OpenDimension, string> = {
  status: 'Open tasks by status',
  assignee: 'Open tasks by assignee',
  priority: 'Open tasks by priority',
  label: 'Open tasks by label',
}

/** The open tasks of a project by one dimension. Status and label rows take their colour from the data. */
export function OpenByChart({ project, by }: { project: Project; by: OpenDimension }) {
  const { workspace } = useWorkspace()
  const [measure] = useMeasure(Boolean(project.estimate_scale))
  const data = useOpenBy(workspace.id, project.id, by).data
  const statuses = useProjectStatuses(workspace.id, project.id).data ?? []
  const members = useMembers(workspace.id).data ?? []
  const labels = useLabels(workspace.id).data ?? []
  const describe = (key: string): { label: string; color?: string } => {
    if (by === 'status') {
      const status = statuses.find((item) => item.id === key)
      return { label: status?.name ?? 'Unknown status', color: status?.color }
    }
    if (by === 'assignee') return { label: key === 'none' ? 'No assignee' : members.find((member) => member.id === key)?.name ?? 'Former member' }
    if (by === 'label') {
      const label = labels.find((item) => item.id === key)
      return key === 'none' ? { label: 'No label' } : { label: label?.name ?? 'Unknown label', color: label?.color }
    }
    return { label: PRIORITY_LABEL[key as keyof typeof PRIORITY_LABEL] ?? key }
  }
  const rows = (data?.items ?? []).map((group) => ({ key: group.key, value: measure === 'points' ? group.points : group.count, ...describe(group.key) }))
  const format = (value: number) => measureText(value, measure)
  return (
    <ChartCard
      title={DIMENSION_TITLE[by]}
      summary={rows.length > 0 ? rows.map((row) => `${row.label}: ${format(row.value)}`).join('; ') : 'No open tasks.'}
      note={unestimatedNote(data?.unestimated_count ?? 0, measure)}
    >
      {rows.length === 0 ? <ChartEmpty>No open tasks.</ChartEmpty> : <BarList rows={rows} format={String} />}
    </ChartCard>
  )
}
