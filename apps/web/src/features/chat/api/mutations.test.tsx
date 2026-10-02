import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import { ChatContext } from './chatContext'
import type { ChatClient } from './client'
import { applyChatEvent, type MessagePages } from './events'
import { chatKeys } from './keys'
import { createMockChatClient } from './mockClient'
import { useSendMessage, useSetFavorite, useToggleReaction } from './mutations'
import { ChatError, type ConversationState } from './types'

const W = 'w1'
const tick = () => new Promise((resolve) => setTimeout(resolve, 5))

/** The provider's wiring without the workspace and auth queries: a client, and its events into a loaded cache. */
async function setup(overrides: Partial<ChatClient> = {}) {
  const mock = createMockChatClient({
    workspaceId: W,
    currentUserId: 'u1',
    members: [
      { id: 'u1', role: 'Member' },
      { id: 'u2', role: 'Owner' },
    ],
    latencyMs: 0,
  })
  const client: ChatClient = { ...mock, ...overrides }
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  mock.subscribe((event) => applyChatEvent(queryClient, W, event))
  const general = (await mock.listConversations()).find((conversation) => conversation.isDefault)!
  queryClient.setQueryData<MessagePages>(chatKeys.messages(W, general.id), {
    pages: [await mock.listMessages(general.id)],
    pageParams: [{}],
  })
  queryClient.setQueryData(chatKeys.states(W), await mock.listStates())
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <ChatContext.Provider value={{ client, workspaceId: W, currentUserId: 'u1' }}>{children}</ChatContext.Provider>
    </QueryClientProvider>
  )
  const messages = () => queryClient.getQueryData<MessagePages>(chatKeys.messages(W, general.id))!.pages.flatMap((page) => page.items)
  const favorite = () =>
    queryClient.getQueryData<ConversationState[]>(chatKeys.states(W))!.find((state) => state.conversationId === general.id)!.favorite
  return { mock, wrapper, general, messages, favorite }
}

function gate() {
  let open = () => {}
  const opened = new Promise<void>((resolve) => (open = resolve))
  return { open, opened }
}

test('a sent message shows at once as sending and is replaced by the confirmed one', async () => {
  const server = gate()
  const { mock, wrapper, general, messages } = await setup({
    sendMessage: async (input) => {
      await server.opened
      return mock.sendMessage(input)
    },
  })
  const count = messages().length
  const view = renderHook(() => useSendMessage(), { wrapper })

  await act(async () => {
    const nonce = view.result.current.send({ conversationId: general.id, body: 'hello' })
    await tick()
    expect(messages().at(-1)).toMatchObject({ body: 'hello', authorId: 'u1', nonce, sendState: 'sending' })
    expect(messages().length).toBe(count + 1)
    server.open()
    await tick()
    expect(messages().length).toBe(count + 1)
    expect(messages().at(-1)).toMatchObject({ body: 'hello', nonce })
    expect(messages().at(-1)?.sendState).toBeUndefined()
    expect(messages().at(-1)?.id.startsWith('m')).toBe(true)
  })
})

test('a failed send stays as failed; retry sends it again and discard removes it', async () => {
  let fail = true
  const { mock, wrapper, general, messages } = await setup({
    sendMessage: async (input) => {
      if (fail) throw new ChatError('offline', 'No connection.')
      return mock.sendMessage(input)
    },
  })
  const count = messages().length
  const view = renderHook(() => useSendMessage(), { wrapper })

  await act(async () => {
    view.result.current.send({ conversationId: general.id, body: 'first' })
    await tick()
    view.result.current.send({ conversationId: general.id, body: 'second' })
    await tick()
  })
  expect(messages().slice(count).map((message) => [message.body, message.sendState])).toEqual([
    ['first', 'failed'],
    ['second', 'failed'],
  ])

  await act(async () => {
    view.result.current.discard(messages().at(-1)!)
    fail = false
    view.result.current.retry(messages().at(-1)!)
    await tick()
  })
  expect(messages().slice(count).map((message) => [message.body, message.sendState])).toEqual([['first', undefined]])
})

test('a reaction and a favorite show at once and roll back when the write fails', async () => {
  const server = gate()
  const refuse = async () => {
    await server.opened
    throw new ChatError('offline', 'No connection.')
  }
  const { wrapper, general, messages, favorite } = await setup({ setReaction: refuse, setFavorite: refuse })
  const message = messages()[0]
  const view = renderHook(() => ({ react: useToggleReaction(), favorite: useSetFavorite() }), { wrapper })

  await act(async () => {
    view.result.current.react.mutate({ message, emoji: '🎉' })
    view.result.current.favorite.mutate({ conversationId: general.id, favorite: true })
    await tick()
    expect(messages()[0].reactions).toContainEqual({ emoji: '🎉', userIds: ['u1'] })
    expect(favorite()).toBe(true)
    server.open()
    await tick()
  })
  expect(messages()[0].reactions).toEqual(message.reactions)
  expect(favorite()).toBe(false)
})
