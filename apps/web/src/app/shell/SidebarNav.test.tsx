import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router'
import { SidebarNav } from './SidebarNav'

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

test('the task navigation includes a current calendar week view', () => {
  const view = render(
    <MemoryRouter initialEntries={['/tasks']}>
      <SidebarNav />
      <Location />
    </MemoryRouter>,
  )

  fireEvent.click(view.getByRole('link', { name: 'This week' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?view=current_week')
  expect(view.getByRole('link', { name: 'This week' }).classList.contains('bg-sidebar-accent')).toBe(true)
  fireEvent.click(view.getByRole('link', { name: 'My week' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?view=my_week')
  expect(view.getByRole('link', { name: 'My week' }).classList.contains('bg-sidebar-accent')).toBe(true)
})

test('task views and the inbox keep the selected project', () => {
  const view = render(
    <MemoryRouter initialEntries={['/tasks?project=project-r']}>
      <SidebarNav />
      <Location />
    </MemoryRouter>,
  )

  fireEvent.click(view.getByRole('link', { name: 'My tasks' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?project=project-r&view=mine')

  fireEvent.click(view.getByRole('link', { name: 'Inbox' }))
  expect(view.getByTestId('location').textContent).toBe('/inbox?project=project-r')

  fireEvent.click(view.getByRole('link', { name: 'This week' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?project=project-r&view=current_week')

  fireEvent.click(view.getByRole('link', { name: 'My week' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?project=project-r&view=my_week')

  fireEvent.click(view.getByRole('link', { name: 'Tasks' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?project=project-r')
})

test('settings pages are not duplicated in the main sidebar', () => {
  const view = render(
    <MemoryRouter>
      <SidebarNav />
    </MemoryRouter>,
  )

  expect(view.getByRole('link', { name: 'Settings' })).toBeTruthy()
  expect(view.queryByRole('link', { name: 'Members' })).toBeNull()
  expect(view.queryByRole('link', { name: 'Sessions' })).toBeNull()
})

test('docs are enabled in the sidebar and highlight on page routes', () => {
  const view = render(
    <MemoryRouter initialEntries={['/tasks']}>
      <SidebarNav />
      <Location />
    </MemoryRouter>,
  )

  const docs = view.getByRole('link', { name: 'Docs' })
  expect(docs.textContent).not.toContain('Coming soon')
  fireEvent.click(docs)
  expect(view.getByTestId('location').textContent).toBe('/docs')
  expect(view.getByRole('link', { name: 'Docs' }).classList.contains('bg-sidebar-accent')).toBe(true)
  for (const label of ['Mail', 'Chat']) {
    expect(view.queryByRole('link', { name: label })).toBeNull()
  }
})
