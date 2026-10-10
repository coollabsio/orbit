import { describe, expect, test } from 'bun:test'
import type { Project } from '@/features/tasks/api/models'
import type { Milestone } from '@/features/tasks/api/milestones'
import { activeMilestone, applyMilestoneDrag, buildRoadmapRows, dayInputToIso, dayToIso, isoToDayInput, milestoneSpan, projectProgress } from './roadmapLib'

const day = (value: string) => dayToIso(new Date(`${value}T00:00:00`))

const milestone = (id: string, fields: Partial<Milestone> = {}): Milestone => ({
  id, workspace_id: 'w', project_id: 'p1', name: id, status: 'planned', start_at: null, target_at: null,
  description_page_id: null, position: 0, completed_at: null, task_count: 0, task_done_count: 0, health: null,
  version: 0, created_at: '', updated_at: '', ...fields,
})

const project = (id: string, name: string): Project => ({
  id, workspace_id: 'w', name, key: name.toUpperCase(), color: '#123456', auto_close_parent: true, auto_close_sub_issues: true,
  triage_enabled: false, lead_user_id: null, member_ids: [], overview_page_id: null, task_counts: {}, version: 0, created_at: '', updated_at: '',
})

describe('milestone dates', () => {
  test('a date input round-trips through the stored day', () => {
    expect(isoToDayInput(dayInputToIso('2026-03-04'))).toBe('2026-03-04')
    expect(dayInputToIso('')).toBeNull()
    expect(isoToDayInput(null)).toBe('')
  })

  test('two dates make a bar; one date makes a point; no date makes nothing', () => {
    expect(milestoneSpan(milestone('a', { start_at: day('2026-03-01'), target_at: day('2026-03-10') }))?.point).toBe(false)
    expect(milestoneSpan(milestone('a', { target_at: day('2026-03-10') }))).toMatchObject({ point: true })
    expect(milestoneSpan(milestone('a', { start_at: day('2026-03-01') }))).toMatchObject({ point: true })
    expect(milestoneSpan(milestone('a'))).toBeNull()
  })
})

describe('applyMilestoneDrag', () => {
  const bar = milestone('a', { start_at: day('2026-03-01'), target_at: day('2026-03-10') })

  test('move shifts both dates', () => {
    expect(applyMilestoneDrag(bar, 'move', 3)).toEqual({ start_at: day('2026-03-04'), target_at: day('2026-03-13') })
  })

  test('an edge stops at the other edge', () => {
    expect(applyMilestoneDrag(bar, 'start', 30)).toEqual({ start_at: day('2026-03-10'), target_at: day('2026-03-10') })
    expect(applyMilestoneDrag(bar, 'end', -30)).toEqual({ start_at: day('2026-03-01'), target_at: day('2026-03-01') })
    expect(applyMilestoneDrag(bar, 'end', 2)).toEqual({ start_at: day('2026-03-01'), target_at: day('2026-03-12') })
  })

  test('a target-only point moves its target, and its start grip pulled earlier adds a start', () => {
    const point = milestone('a', { target_at: day('2026-03-10') })
    expect(applyMilestoneDrag(point, 'move', -2)).toEqual({ start_at: null, target_at: day('2026-03-08') })
    expect(applyMilestoneDrag(point, 'start', -4)).toEqual({ start_at: day('2026-03-06'), target_at: day('2026-03-10') })
    expect(applyMilestoneDrag(point, 'start', 4)).toEqual(point)
  })

  test('a start-only point moves its start, and its end grip pulled later adds a target', () => {
    const point = milestone('a', { start_at: day('2026-03-10') })
    expect(applyMilestoneDrag(point, 'move', 1)).toEqual({ start_at: day('2026-03-11'), target_at: null })
    expect(applyMilestoneDrag(point, 'end', 5)).toEqual({ start_at: day('2026-03-10'), target_at: day('2026-03-15') })
  })

  test('a milestone with no dates cannot be dragged', () => {
    expect(applyMilestoneDrag(milestone('a'), 'move', 1)).toBeNull()
  })
})

describe('buildRoadmapRows', () => {
  test('groups by project name, orders dated milestones by start and lists undated ones last', () => {
    const rows = buildRoadmapRows(
      [project('p2', 'Zeta'), project('p1', 'Alpha'), project('p3', 'Empty')],
      [
        milestone('late', { start_at: day('2026-05-01'), target_at: day('2026-05-09') }),
        milestone('early', { target_at: day('2026-03-01') }),
        milestone('undated'),
        milestone('zeta', { project_id: 'p2', target_at: day('2026-01-01') }),
      ],
    )
    expect(rows.map((row) => row.key)).toEqual(['project:p1', 'early', 'late', 'undated:p1', 'project:p2', 'zeta'])
    const undated = rows[3]
    expect(undated?.kind === 'undated' ? undated.milestones.map((item) => item.id) : null).toEqual(['undated'])
  })
})

describe('project summary', () => {
  test('the active milestone is the one in progress, else the next planned one by target date', () => {
    const planned = milestone('planned', { target_at: day('2026-04-01') })
    const sooner = milestone('sooner', { target_at: day('2026-03-01') })
    const running = milestone('running', { status: 'in_progress', target_at: day('2026-09-01') })
    const done = milestone('done', { status: 'completed' })
    expect(activeMilestone([planned, sooner, running, done])?.id).toBe('running')
    expect(activeMilestone([planned, sooner, milestone('undated'), done])?.id).toBe('sooner')
    expect(activeMilestone([done])).toBeUndefined()
  })

  test('progress leaves cancelled and duplicate tasks out of the total', () => {
    expect(projectProgress({ unstarted: 2, started: 1, completed: 3, cancelled: 4, duplicate: 5 })).toEqual({ done: 3, total: 6 })
    expect(projectProgress({})).toEqual({ done: 0, total: 0 })
  })
})
