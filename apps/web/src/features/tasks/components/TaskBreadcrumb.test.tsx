import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { Project, TaskKeyRef } from '@/features/tasks/api/models'
import { TaskBreadcrumb } from './TaskBreadcrumb'

const ref = (n: number): TaskKeyRef => ({ id: `task-000${n}`, title: `Level ${n}`, projectKey: 'ORB' })

test('ancestors link root first; more than three collapse the middle into a menu', async () => {
  const opened: string[] = []
  const view = render(<TaskBreadcrumb ancestors={[1, 2, 3, 4, 5].map(ref)} identifier="ORB-0031" onOpen={(id) => opened.push(id)} />)
  const nav = view.getByRole('navigation', { name: 'breadcrumb' })
  expect(nav.textContent).toContain('ORB-0001')
  expect(nav.textContent).toContain('ORB-0005')
  expect(nav.textContent).not.toContain('ORB-0003')
  expect(nav.textContent!.endsWith('ORB-0031')).toBe(true)
  fireEvent.click(view.getByRole('button', { name: /ORB-0001/ }))
  expect(opened).toEqual(['task-0001'])
  fireEvent.click(view.getByRole('button', { name: 'Show 3 more parents' }))
  await userEvent.click(await view.findByRole('menuitem', { name: /ORB-0003/ }))
  expect(opened).toEqual(['task-0001', 'task-0003'])
})

test('a top-level task shows only its own identifier', () => {
  const view = render(<TaskBreadcrumb ancestors={[]} identifier="ORB-0031" onOpen={() => {}} />)
  expect(view.getByRole('navigation', { name: 'breadcrumb' }).textContent).toBe('ORB-0031')
  expect(view.queryAllByRole('button')).toHaveLength(0)
})

test('the trail starts with the project, which opens its task list', () => {
  const opened: string[] = []
  const project = { id: 'project-1', key: 'ORB', name: 'Orbit', color: '#f2458f' } as Project
  const view = render(<TaskBreadcrumb project={project} ancestors={[ref(1)]} identifier="ORB-0031" onOpen={() => {}} onOpenProject={(id) => opened.push(id)} />)
  expect(view.getByRole('navigation', { name: 'breadcrumb' }).textContent!.startsWith('Orbit')).toBe(true)
  fireEvent.click(view.getByRole('button', { name: 'Orbit' }))
  expect(opened).toEqual(['project-1'])
})
