import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query'
import { act, renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import { ChatContext } from './chatContext'
import type { ChatClient, SendMessageInput } from './client'
import { applyChatEvent, type MessagePages } from './events'
import { chatKeys } from './keys'
import { useSendMessage, useSetFavorite, useToggleReaction } from './mutations'
import { ChatError, type Conversation, type ConversationState, type Message } from './types'

const W = 'w1'
const tick = () => new Promise((resolve) => setTimeout(resolve, 5))

const general: Conversation = {
  id: 'c1',
  kind: 'public',
  name: 'general',
  topic: '',
  categoryId: null,
  position: 0,
  memberIds: ['u1', 'u2'],
  isMember: true,
  isDefault: true,
  selfDm: false,
  archived: false,
  createdBy: 'u2',
  createdAt: 1,
  lastMessageAt: 1,
}

function message(id: string, authorId: string, body: string, nonce: string | null = null): Message {
  return {
    id,
    conversationId: general.id,
    threadRootId: null,
    kind: 'message',
    authorId,
    body,
    mentions: { userIds: [], channel: false, here: false },
    createdAt: 1,
    editedAt: null,
    deleted: false,
    attachments: [],
    reactions: [],
    pinned: false,
    alsoInChannel: false,
    nonce,
    replyCount: 0,
    lastReplyAt: null,
    replyUserIds: [],
    lastReply: null,
  }
}

/**
 * The provider's wiring without the workspace and auth queries: a loaded cache, and a client that confirms a send the
 * way the server does (the message comes back as an event and as the result). `overrides` make single calls fail.
 */
async function setup(overrides: Partial<ChatClient> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  let sent = 0
  const server = {
    async sendMessage(input: SendMessageInput) {
      const confirmed = message(`m${++sent}`, 'u1', input.body, input.nonce)
      applyChatEvent(queryClient, W, { type: 'message.created', message: confirmed })
      return confirmed
    },
  }
  const client = { ...server, ...overrides } as ChatClient
  queryClient.setQueryData<MessagePages>(chatKeys.messages(W, general.id), {
    pages: [{ items: [message('m0', 'u2', 'Welcome')], before: null, after: null }],
    pageParams: [{}],
  })
  const state: ConversationState = { conversationId: general.id, lastReadMessageId: 'm0', unreadCount: 0, mentionCount: 0, notify: 'mentions', favorite: false }
  queryClient.setQueryData(chatKeys.states(W), [state])
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <ChatContext.Provider value={{ client, workspaceId: W, currentUserId: 'u1' }}>{children}</ChatContext.Provider>
    </QueryClientProvider>
  )
  /** As the list shows them: the confirmed messages, then the outbox. */
  const messages = () => [
    ...queryClient.getQueryData<MessagePages>(chatKeys.messages(W, general.id))!.pages.flatMap((page) => page.items),
    ...(queryClient.getQueryData<Message[]>(chatKeys.outbox(W)) ?? []),
  ]
  const favorite = () =>
    queryClient.getQueryData<ConversationState[]>(chatKeys.states(W))!.find((state) => state.conversationId === general.id)!.favorite
  return { mock: server, wrapper, general, messages, favorite }
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

test('a failed reaction does not come back when the server’s message came meanwhile', async () => {
  const server = gate()
  const { wrapper, messages } = await setup({
    setReaction: async () => {
      await server.opened
      throw new ChatError('conflict', 'Too many reactions.')
    },
  })
  const message = messages()[0]
  const view = renderHook(() => ({ react: useToggleReaction(), client: useQueryClient() }), { wrapper })

  await act(async () => {
    view.result.current.react.mutate({ message, emoji: '🎉' })
    await tick()
    // An event with the message as the server has it: without the reaction.
    applyChatEvent(view.result.current.client, W, { type: 'message.updated', message })
    server.open()
    await tick()
  })
  expect(messages()[0].reactions).toEqual(message.reactions)
})
