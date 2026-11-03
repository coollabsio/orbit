import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router'
import { ActivityBell } from './ActivityBell'

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

test('the bell opens Activity and keeps the selected project', () => {
  const view = render(
    <MemoryRouter initialEntries={['/tasks?project=project-r&view=mine']}>
      <ActivityBell />
      <Location />
    </MemoryRouter>,
  )
  const bell = view.getByRole('link', { name: 'Activity' })
  expect(bell.dataset.active).toBe('false')

  fireEvent.click(bell)
  expect(view.getByTestId('location').textContent).toBe('/activity?project=project-r')
  expect(bell.getAttribute('aria-current')).toBe('page')
})

test('the bell shows a dot while a notification is unread', () => {
  const view = render(
    <MemoryRouter>
      <ActivityBell unread />
    </MemoryRouter>,
  )
  expect(view.getByRole('link', { name: 'Activity, unread' }).querySelectorAll('[data-slot="activity-bell-dot"]').length).toBe(1)

  view.rerender(
    <MemoryRouter>
      <ActivityBell />
    </MemoryRouter>,
  )
  expect(view.getByRole('link', { name: 'Activity' }).querySelectorAll('[data-slot="activity-bell-dot"]').length).toBe(0)
})
