import { expect, test } from 'bun:test'

const source = await Bun.file(new URL('./App.tsx', import.meta.url)).text()

test('every chat URL renders the one chat page, which reads the params itself', () => {
  for (const path of ['chat', 'chat/unreads', 'chat/threads', 'chat/:conversationId', 'chat/:conversationId/thread/:messageId']) {
    expect(source).toContain(`<Route path="${path}" element={<ChatPage />} />`)
  }
})

test('the chat routes exist in development builds only; production still sends chat to tasks', () => {
  const enabled = source.indexOf('{chatEnabled ? (')
  const fallback = source.indexOf('<Route path="chat/*" element={<Navigate to="/tasks" replace />} />')
  expect(enabled).toBeGreaterThan(-1)
  expect(source.indexOf('<Route path="chat" element={<ChatPage />} />')).toBeGreaterThan(enabled)
  expect(fallback).toBeGreaterThan(source.indexOf('<Route path="chat/:conversationId/thread/:messageId"'))
})

test('old direct message links go to chat, or to tasks in production', () => {
  expect(source).toContain(`<Route path="dm/*" element={<Navigate to={chatEnabled ? '/chat' : '/tasks'} replace />} />`)
  expect(source).not.toContain('DMPage')
})
