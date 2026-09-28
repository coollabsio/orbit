import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import { SubIssueProgress, completedStatusColor } from './SubIssueProgress'

const status = (id: string, category: TaskStatusDef['category'], color: string, position: number): TaskStatusDef =>
  ({ id, projectId: 'project-1', name: id, description: '', color, category, position, version: 1 })

test('the ring shows closed/total and draws the closed share in the given colour', () => {
  const view = render(<SubIssueProgress closed={2} total={5} color="#4cb782" />)
  const chip = view.getByRole('img', { name: '2 of 5 sub-issues closed' })
  expect(chip.textContent).toBe('2/5')
  const arc = chip.querySelector('[data-slot="progress-arc"]')!
  expect(arc.getAttribute('stroke')).toBe('#4cb782')
  expect(Number(arc.getAttribute('stroke-dashoffset'))).toBeCloseTo(2 * Math.PI * 5.25 * 0.6)
  expect(arc.getAttribute('class')).toContain('duration-200')
})

test('the arc colour is the project\'s first completed status, else the default done green', () => {
  const statuses = [status('todo', 'unstarted', '#888', 0), status('shipped', 'completed', '#123456', 2), status('done', 'completed', '#abcdef', 3)]
  expect(completedStatusColor(statuses, 'project-1')).toBe('#123456')
  expect(completedStatusColor([], 'project-1')).toBe('#4cb782')
})
