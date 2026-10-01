import { expect, test } from 'bun:test'
import { fireEvent, render, within } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router'
import { MobileDock } from './MobileDock'

function Location() {
  return <output data-testid="location">{useLocation().pathname}</output>
}

test('upcoming mobile sections are visible but cannot navigate', () => {
  const view = render(<MemoryRouter initialEntries={['/tasks']}><MobileDock chatEnabled={false} /><Location /></MemoryRouter>)
  const dock = within(view.getByRole('navigation'))
  for (const label of ['Home', 'Mail', 'Chat']) {
    const button = dock.getByRole('button', { name: label }) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    expect(button.textContent).not.toContain('Soon')
    fireEvent.click(button)
    expect(view.getByTestId('location').textContent).toBe('/tasks')
  }
  expect(dock.queryAllByText('DMs')).toHaveLength(0)
  expect(dock.getAllByRole('link')).toHaveLength(3)
  fireEvent.click(dock.getByRole('link', { name: 'Docs' }))
  expect(view.getByTestId('location').textContent).toBe('/docs')
  fireEvent.click(dock.getByRole('link', { name: 'Settings' }))
  expect(view.getByTestId('location').textContent).toBe('/settings')
  fireEvent.click(dock.getByRole('link', { name: 'Tasks' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks')
})

test('in a development build Chat is a dock link', () => {
  const view = render(<MemoryRouter initialEntries={['/tasks']}><MobileDock chatEnabled /><Location /></MemoryRouter>)
  const dock = within(view.getByRole('navigation'))
  expect(dock.getAllByRole('link')).toHaveLength(4)
  fireEvent.click(dock.getByRole('link', { name: 'Chat' }))
  expect(view.getByTestId('location').textContent).toBe('/chat')
  // the chat list keeps the dock
  expect(view.getAllByRole('navigation')).toHaveLength(1)
})

test('the dock is hidden inside a chat conversation', () => {
  for (const path of ['/chat/c1', '/chat/c1/thread/m1', '/chat/unreads', '/chat/threads']) {
    const view = render(<MemoryRouter initialEntries={[path]}><MobileDock chatEnabled /></MemoryRouter>)
    expect(view.queryAllByRole('navigation')).toHaveLength(0)
    view.unmount()
  }
})

test('the Chat dock link shows the unread count and names it', () => {
  const view = render(<MemoryRouter initialEntries={['/tasks']}><MobileDock chatEnabled chatBadge={3} /></MemoryRouter>)
  const dock = within(view.getByRole('navigation'))
  expect(dock.getByRole('link', { name: 'Chat, 3 unread' }).textContent).toContain('3')
  view.unmount()
  // nothing shows for zero, and never on the disabled entry
  const none = render(<MemoryRouter initialEntries={['/tasks']}><MobileDock chatEnabled={false} chatBadge={3} /></MemoryRouter>)
  expect(none.container.querySelectorAll('[data-slot="mobile-dock-badge"]')).toHaveLength(0)
})
