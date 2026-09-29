import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { getState } from '@/mock/store'
import type { AppState, ChatMessage } from '@/mock/types'
import { MessageList } from './MessageList'

function rows(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>('[data-message-index]'))
}

function channelWithMessages(state: AppState) {
  return state.channels.find((c) => state.chatMessages.some((m) => m.channelId === c.id && !m.threadRootId))!
}

function list(state: AppState) {
  const channel = channelWithMessages(state)
  return (
    <MemoryRouter>
      <MessageList state={state} channel={channel} onReply={() => {}} onOpenThread={() => {}} />
    </MemoryRouter>
  )
}

test('only messages that arrive after mount get the enter animation', () => {
  const state = getState()
  const view = render(list(state))
  const initial = rows(view.container)
  expect(initial.length).toBeGreaterThan(0)
  expect(initial.every((row) => !row.hasAttribute('data-new'))).toBe(true)

  const channelId = channelWithMessages(state).id
  const template = state.chatMessages.find((m) => m.channelId === channelId && !m.threadRootId)!
  const oldest = state.chatMessages.filter((m) => m.channelId === channelId).map((m) => m.createdAt).sort()[0]
  const arrived: ChatMessage = { ...template, id: 'arrived-after-mount', content: 'hello', reactions: [], pinned: false, createdAt: new Date(Date.now() + 60_000).toISOString() }
  // an older page loading in (pagination) must not animate
  const older: ChatMessage = { ...template, id: 'older-page', content: 'old', reactions: [], pinned: false, createdAt: new Date(new Date(oldest).getTime() - 60_000).toISOString() }
  view.rerender(list({ ...state, chatMessages: [...state.chatMessages, arrived, older] }))

  const after = rows(view.container)
  const animated = after.filter((row) => row.dataset.new === 'true')
  expect(animated).toHaveLength(1)
  expect(animated[0].querySelector('#message-arrived-after-mount')).not.toBeNull()
  expect(after.some((row) => row.querySelector('#message-older-page'))).toBe(true)
  expect(after.every((row) => !row.style.animationDelay)).toBe(true)
})
