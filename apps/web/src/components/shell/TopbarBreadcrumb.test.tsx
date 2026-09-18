import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { TopbarBreadcrumb } from './TopbarBreadcrumb'

test('every page-supplied crumb is preceded by a separator so it reads after the workspace root', () => {
  const view = render(
    <MemoryRouter>
      <TopbarBreadcrumb crumbs={[{ label: 'ORB-9', to: '/tasks/task-9' }, { label: 'ORB-12' }]} />
    </MemoryRouter>,
  )

  expect(view.container.querySelectorAll('.topbar-crumb-sep')).toHaveLength(2)
  expect(view.container.querySelectorAll('.topbar-crumb-sep')[0].textContent).toBe('›')
})

test('only non-terminal crumbs with a target become links', () => {
  const view = render(
    <MemoryRouter>
      <TopbarBreadcrumb crumbs={[{ label: 'ORB-9', to: '/tasks/task-9' }, { label: 'ORB-12', to: '/tasks/task-12' }]} />
    </MemoryRouter>,
  )

  const link = view.getByRole('link', { name: 'ORB-9' }) as HTMLAnchorElement
  expect(link.getAttribute('href')).toBe('/tasks/task-9')
  expect(view.queryByRole('link', { name: 'ORB-12' })).toBeNull()
  expect((view.getByText('ORB-12') as HTMLElement).dataset.current).toBe('true')
})

test('a single crumb renders as the current segment', () => {
  const view = render(<MemoryRouter><TopbarBreadcrumb crumbs={[{ label: 'Settings' }]} /></MemoryRouter>)

  expect(view.container.querySelectorAll('.topbar-crumb')).toHaveLength(1)
  expect((view.getByText('Settings') as HTMLElement).dataset.current).toBe('true')
})
