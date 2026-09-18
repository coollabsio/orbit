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
  expect(view.getByRole('link', { name: 'This week' }).classList.contains('active')).toBe(true)
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
