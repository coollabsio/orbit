import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useState } from 'react'
import { Link, MemoryRouter, useLocation } from 'react-router'
import { InAppSidebar } from './InAppSidebar'
import { SidebarSlotContext } from './sidebarSlot'
import { SidebarNav } from './SidebarNav'

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

test('the task navigation includes a current calendar week view', () => {
  const view = render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/tasks']}>
        <SidebarNav />
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )

  fireEvent.click(view.getByRole('link', { name: 'This week' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?view=current_week')
  expect(view.getByRole('link', { name: 'This week' }).dataset.active).toBe('true')
  fireEvent.click(view.getByRole('link', { name: 'My week' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?view=my_week')
  expect(view.getByRole('link', { name: 'My week' }).dataset.active).toBe('true')
})

test('task views keep the selected project', () => {
  const view = render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/tasks?project=project-r']}>
        <SidebarNav />
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )

  fireEvent.click(view.getByRole('link', { name: 'My tasks' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?project=project-r&view=mine')

  fireEvent.click(view.getByRole('link', { name: 'This week' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?project=project-r&view=current_week')

  fireEvent.click(view.getByRole('link', { name: 'My week' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?project=project-r&view=my_week')

  fireEvent.click(view.getByRole('link', { name: 'All tasks' }))
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
    <MemoryRouter initialEntries={['/']}>
      <SidebarNav mailEnabled />
      <Location />
    </MemoryRouter>,
  )

  const docs = view.getByRole('link', { name: 'Docs' })
  expect(docs.textContent).not.toContain('Coming soon')
  fireEvent.click(docs)
  expect(view.getByTestId('location').textContent).toBe('/docs')
  expect(view.getByRole('link', { name: 'Docs' }).dataset.active).toBe('true')
  // Mail and Chat are products; Mail carries a Beta tag.
  const mail = view.getByRole('link', { name: 'Mail' })
  expect(mail.querySelector('[data-slot="sidebar-nav-tag"]')?.textContent).toBe('Beta')
  fireEvent.click(mail)
  expect(view.getByTestId('location').textContent).toBe('/mail')
  expect(view.getByRole('link', { name: 'Chat' })).toBeTruthy()
})

test('Mail is Coming soon in the sidebar while Mail is off for the server', () => {
  const view = render(<MemoryRouter initialEntries={['/']}><SidebarNav /></MemoryRouter>)
  expect(view.queryByRole('link', { name: 'Mail' })).toBeNull()
  const mail = view.getByRole('button', { name: /Mail/ }) as HTMLButtonElement
  expect(mail.disabled).toBe(true)
  expect(mail.textContent).toContain('Coming soon')
})

test('chat and direct messages are not products in production: Chat is "Coming soon"', () => {
  const view = render(
    <MemoryRouter initialEntries={['/']}>
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
    <MemoryRouter initialEntries={['/']}>
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
      <MemoryRouter initialEntries={['/']}>
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

test('inside account settings the sidebar shows its links, and Back returns to the last app page', () => {
  const view = render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/tasks?view=mine']}>
        <SidebarNav />
        <Link to="/profile/appearance">Open appearance</Link>
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )

  const enter = () => (view.container.querySelector('[data-slot=sidebar-nav-content]') as HTMLElement).dataset.enter
  expect(enter()).toBeUndefined()

  fireEvent.click(view.getByText('Open appearance'))
  expect(enter()).toBe('forward')
  expect(view.getByRole('link', { name: 'Back to the main menu' }).textContent).toBe('Account settings')
  expect(view.queryByRole('link', { name: 'Tasks' })).toBeNull()
  expect(view.getByRole('link', { name: 'Appearance' }).dataset.active).toBe('true')
  fireEvent.click(view.getByRole('link', { name: 'Sessions' }))
  expect(view.getByTestId('location').textContent).toBe('/profile/sessions')

  fireEvent.click(view.getByRole('link', { name: 'Back to the main menu' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?view=mine')
  expect(enter()).toBe('back')
  expect(view.getByRole('link', { name: 'My tasks' }).dataset.active).toBe('true')
})

/** The desktop shell in small: an open sidebar that holds the page's list, and a page that renders one. */
function ShellWithPageList() {
  const [slot, setSlot] = useState<HTMLElement | null>(null)
  return (
    <SidebarSlotContext value={slot}>
      <aside data-testid="sidebar">
        <SidebarNav pageListRef={setSlot} />
      </aside>
      <main data-testid="page">
        <InAppSidebar>
          <nav aria-label="Pages">page tree</nav>
        </InAppSidebar>
      </main>
      <Link to="/docs/page-1">Open docs</Link>
      <Location />
    </SidebarSlotContext>
  )
}

test('on docs the open sidebar holds the page tree instead of the app links, and Back leaves docs', () => {
  const view = render(
    <MemoryRouter initialEntries={['/activity']}>
      <ShellWithPageList />
    </MemoryRouter>,
  )
  expect(view.getByTestId('page').textContent).toBe('page tree')

  fireEvent.click(view.getByText('Open docs'))

  expect(view.getByTestId('sidebar').contains(view.getByRole('navigation', { name: 'Pages' }))).toBe(true)
  expect(view.getByTestId('page').textContent).toBe('')
  expect(view.getByRole('link', { name: 'Back to the main menu' }).textContent).toBe('Docs')
  expect(view.queryByRole('link', { name: 'Tasks' })).toBeNull()

  fireEvent.click(view.getByRole('link', { name: 'Back to the main menu' }))
  expect(view.getByTestId('location').textContent).toBe('/activity')
  expect(view.getByRole('link', { name: 'Tasks' })).toBeTruthy()
})

test('Back pops: from docs to account settings and back twice, it never loops between them', () => {
  const view = render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/activity']}>
        <ShellWithPageList />
        <Link to="/profile/appearance">Open appearance</Link>
      </MemoryRouter>
    </QueryClientProvider>,
  )
  const back = () => fireEvent.click(view.getByRole('link', { name: 'Back to the main menu' }))
  const location = () => view.getByTestId('location').textContent

  fireEvent.click(view.getByText('Open docs'))
  fireEvent.click(view.getByText('Open appearance'))
  back()
  expect(location()).toBe('/docs/page-1')
  back()
  expect(location()).toBe('/activity')
  expect(view.queryByRole('link', { name: 'Back to the main menu' })).toBeNull()
})

test('in the mobile drawer Back shows the main menu and stays on the page', () => {
  const view = render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/tasks', '/profile/appearance']} initialIndex={1}>
        <SidebarNav backShowsMenu />
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  fireEvent.click(view.getByRole('button', { name: 'Back to the main menu' }))
  expect(view.getByTestId('location').textContent).toBe('/profile/appearance')
  expect(view.getByRole('link', { name: 'Tasks' })).toBeTruthy()
  expect(view.queryByRole('link', { name: 'Appearance' })).toBeNull()
})

test('Tasks has its own area: views, task pages and projects leave the main menu', () => {
  const view = render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/']}>
        <SidebarNav />
        <Link to="/tasks?view=overdue">Open overdue</Link>
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  expect(view.queryByRole('link', { name: 'Task views' })).toBeNull()
  expect(view.queryByRole('link', { name: 'My tasks' })).toBeNull()

  fireEvent.click(view.getByText('Open overdue'))
  expect(view.getByRole('link', { name: 'Back to the main menu' }).textContent).toBe('Tasks')
  expect(view.getByRole('link', { name: 'Overdue' }).dataset.active).toBe('true')
  fireEvent.click(view.getByRole('link', { name: 'Task views' }))
  expect(view.getByTestId('location').textContent).toBe('/views')
  expect(view.getByRole('link', { name: 'Back to the main menu' }).textContent).toBe('Tasks')
})

test('the task trash sits in the Tasks area, which stays open while the trash is shown', () => {
  const view = render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/tasks']}>
        <SidebarNav />
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )

  fireEvent.click(view.getByRole('link', { name: 'Task trash' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks-trash')
  expect(view.getByRole('link', { name: 'Back to the main menu' }).textContent).toBe('Tasks')
  expect(view.getByRole('link', { name: 'Task trash' }).dataset.active).toBe('true')
})
