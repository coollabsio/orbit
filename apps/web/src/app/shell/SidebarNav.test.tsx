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
  expect(view.getByRole('link', { name: 'This week' }).dataset.active).toBe('true')
  fireEvent.click(view.getByRole('link', { name: 'My week' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?view=my_week')
  expect(view.getByRole('link', { name: 'My week' }).dataset.active).toBe('true')
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

test('task views nest under Tasks, not beside the other apps', () => {
  const view = render(
    <MemoryRouter initialEntries={['/tasks']}>
      <SidebarNav />
      <Location />
    </MemoryRouter>,
  )
  const links = view.getAllByRole('link').map((link) => link.getAttribute('aria-label'))
  expect(links.indexOf('Task views')).toBe(links.indexOf('Tasks') + 1)
  expect(view.queryByRole('link', { name: 'Views' })).toBeNull()
  fireEvent.click(view.getByRole('link', { name: 'Task views' }))
  expect(view.getByTestId('location').textContent).toBe('/views')
  expect(view.getByRole('link', { name: 'Task views' }).dataset.active).toBe('true')
  expect(view.getByRole('link', { name: 'Tasks' }).dataset.active).toBe('false')
})

test('the views entry shows only while the user is in Tasks', () => {
  const view = render(
    <MemoryRouter initialEntries={['/settings']}>
      <SidebarNav />
    </MemoryRouter>,
  )
  expect(view.queryByRole('link', { name: 'Task views' })).toBeNull()
  view.unmount()
  for (const path of ['/views', '/views/view-1', '/tasks/task-1', '/tasks-trash']) {
    const inside = render(
      <MemoryRouter initialEntries={[path]}>
        <SidebarNav />
      </MemoryRouter>,
    )
    expect(inside.getAllByRole('link', { name: 'Task views' })).toHaveLength(1)
    inside.unmount()
  }
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
  expect(view.getByRole('link', { name: 'Docs' }).dataset.active).toBe('true')
  // Mail is not a product yet; Chat is.
  expect(view.queryByRole('link', { name: 'Mail' })).toBeNull()
  expect(view.getByRole('link', { name: 'Chat' })).toBeTruthy()
})

test('chat and direct messages are not products in production: Chat is "Coming soon"', () => {
  const view = render(
    <MemoryRouter initialEntries={['/tasks']}>
      <SidebarNav chatEnabled={false} chatBadge={4} />
    </MemoryRouter>,
  )

  expect(view.queryAllByRole('link', { name: /Chat/ })).toHaveLength(0)
  expect((view.getByRole('button', { name: /Chat/ }) as HTMLButtonElement).disabled).toBe(true)
  expect(view.getByRole('button', { name: /Chat/ }).textContent).toContain('Coming soon')
  expect(view.queryAllByText('Direct messages')).toHaveLength(0)
})

test('in a development build Chat is a link that highlights on its routes', () => {
  const view = render(
    <MemoryRouter initialEntries={['/tasks']}>
      <SidebarNav chatEnabled />
      <Location />
    </MemoryRouter>,
  )

  fireEvent.click(view.getByRole('link', { name: 'Chat' }))
  expect(view.getByTestId('location').textContent).toBe('/chat')
  expect(view.getByRole('link', { name: 'Chat' }).dataset.active).toBe('true')
  expect(view.getByRole('link', { name: 'Chat' }).querySelectorAll('[data-slot=sidebar-nav-badge]')).toHaveLength(0)
})

test('the chat count shows on the Chat item, is in its name, stops at 99+ and hides at zero', () => {
  const badgeOf = (count: number | undefined) => {
    const view = render(
      <MemoryRouter initialEntries={['/tasks']}>
        <SidebarNav chatEnabled chatBadge={count} />
      </MemoryRouter>,
    )
    const link = view.getByRole('link', { name: /^Chat/ })
    const result = { name: link.getAttribute('aria-label'), badge: link.querySelector('[data-slot=sidebar-nav-badge]')?.textContent ?? null }
    view.unmount()
    return result
  }

  expect(badgeOf(3)).toEqual({ name: 'Chat, 3 unread', badge: '3' })
  expect(badgeOf(120)).toEqual({ name: 'Chat, 120 unread', badge: '99+' })
  expect(badgeOf(0)).toEqual({ name: 'Chat', badge: null })
  expect(badgeOf(undefined)).toEqual({ name: 'Chat', badge: null })
})
