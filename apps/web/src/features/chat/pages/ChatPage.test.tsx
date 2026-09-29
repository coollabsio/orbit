import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { getState } from '@/mock/store'
import { ChatPage } from './ChatPage'

function renderChat() {
  const channel = getState().channels[0]
  return render(
    <MemoryRouter initialEntries={[`/chat/${channel.id}`]}>
      <Routes>
        <Route path="/chat/:channelId" element={<ChatPage />} />
      </Routes>
    </MemoryRouter>,
  )
}

test('the channel context menu marks delete as a danger item', async () => {
  const view = renderChat()
  const channel = getState().channels[0]
  const row = view.container.querySelector<HTMLElement>(`[data-channel-drop-id="${channel.id}"]`)
  fireEvent.contextMenu(row!)

  const item = await view.findByRole('menuitem', { name: 'Delete Channel' })
  expect(item.getAttribute('data-danger')).toBe('true')
  expect(view.getByRole('menuitem', { name: 'Edit Channel' }).hasAttribute('data-danger')).toBe(false)
})

test('header panel buttons flag the open panel as active', async () => {
  const view = renderChat()
  expect(view.getByTitle('Files').dataset.slot).toBe('chat-icon-button')
  const pins = view.getByTitle('Pinned Messages')
  expect(pins.hasAttribute('data-active')).toBe(false)
  fireEvent.click(pins)
  await view.findByPlaceholderText('Search pinned messages')
  expect(pins.getAttribute('data-active')).toBe('true')
})
