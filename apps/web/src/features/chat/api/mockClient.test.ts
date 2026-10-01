import { expect, test } from 'bun:test'
import type { ChatClient } from './client'
import { createMockChatClient, type MockChatClientOptions } from './mockClient'
import { ChatError, type ChatEvent, type Message } from './types'

const members: MockChatClientOptions['members'] = [
  { id: 'u1', role: 'Member' },
  { id: 'u2', role: 'Owner' },
  { id: 'u3', role: 'Member' },
  { id: 'u4', role: 'Member' },
]

function setup(overrides: Partial<MockChatClientOptions> = {}) {
  const client = createMockChatClient({ workspaceId: 'w1', currentUserId: 'u1', members, latencyMs: 0, ...overrides })
  const events: ChatEvent[] = []
  client.subscribe((event) => events.push(event))
  return { client, events }
}

async function channel(client: ChatClient, name: string) {
  const found = (await client.listConversations()).find((conversation) => conversation.name === name)
  if (!found) throw new Error(`no #${name}`)
  return found
}

async function stateOf(client: ChatClient, conversationId: string) {
  return (await client.listStates()).find((state) => state.conversationId === conversationId)
}

async function allMessages(client: ChatClient, conversationId: string): Promise<Message[]> {
  return (await client.listMessages(conversationId, { limit: 1000 })).items
}

const nextTimers = () => new Promise((resolve) => setTimeout(resolve, 5))
const send = (client: ChatClient, conversationId: string, body: string, extra: { threadRootId?: string; alsoInChannel?: boolean } = {}) =>
  client.sendMessage({ conversationId, body, nonce: crypto.randomUUID(), ...extra })

test('the seed is the same every time, and message ids sort by time', async () => {
  const first = await allMessages(setup().client, (await channel(setup().client, 'general')).id)
  const { client } = setup()
  const general = await channel(client, 'general')
  const second = await allMessages(client, general.id)
  expect(second.map((message) => [message.id, message.authorId, message.body])).toEqual(
    first.map((message) => [message.id, message.authorId, message.body]),
  )
  expect(general.isDefault).toBe(true)
  expect(second.length).toBeGreaterThanOrEqual(150)
  for (let index = 1; index < second.length; index++) {
    if (second[index - 1].id >= second[index].id || second[index - 1].createdAt > second[index].createdAt) {
      throw new Error(`messages out of order at ${index}`)
    }
  }
  expect(new Set(second.map((message) => new Date(message.createdAt).toDateString())).size).toBeGreaterThanOrEqual(3)
})

test('messages page in both directions and around a message', async () => {
  const { client } = setup()
  const general = await channel(client, 'general')
  const all = await allMessages(client, general.id)
  const newest = await client.listMessages(general.id)
  expect(newest.items.length).toBe(50)
  expect(newest.items.at(-1)?.id).toBe(all.at(-1)!.id)
  expect(newest.after).toBeNull()
  const older = await client.listMessages(general.id, { before: newest.before! })
  expect(older.items.at(-1)?.id).toBe(all.at(-51)!.id)
  const newer = await client.listMessages(general.id, { after: older.items.at(-1)!.id })
  expect(newer.items[0].id).toBe(newest.items[0].id)

  const around = await client.listMessages(general.id, { around: all[60].id, limit: 20 })
  expect(around.items.map((message) => message.id)).toEqual(all.slice(50, 70).map((message) => message.id))
  expect(around.before).toBe(all[50].id)
  expect(around.after).toBe(all[69].id)
  const start = await client.listMessages(general.id, { before: all[5].id })
  expect(start.items.length).toBe(5)
  expect(start.before).toBeNull()
})

test('a private channel the user is not in is invisible; an unjoined public channel can be read', async () => {
  const { client } = setup()
  const names = (await client.listConversations()).map((conversation) => conversation.name)
  expect(names).toContain('leadership')
  expect(names).not.toContain('hiring')

  // Its id answers "not found", like an id that never existed: only the visible conversations can be read or joined.
  const visible = new Set((await client.listConversations()).map((conversation) => conversation.id))
  const readableIds: string[] = []
  for (let counter = 1; counter <= 400; counter++) {
    const id = `c${String(counter).padStart(8, '0')}`
    const result = await client.listMessages(id).catch((error: unknown) => error)
    if (result instanceof ChatError) {
      if (result.code !== 'not_found') throw new Error(`unexpected ${result.code}`)
      await expect(client.joinChannel(id)).rejects.toMatchObject({ code: 'not_found' })
    } else {
      readableIds.push(id)
    }
  }
  expect(new Set(readableIds)).toEqual(visible)
  expect((await client.searchMessages({ query: 'interviews' })).items).toEqual([])
  expect((await client.searchMessages({ query: 'launch video' })).items.length).toBe(1)

  const marketing = await channel(client, 'marketing')
  expect(marketing.isMember).toBe(false)
  expect((await client.listMessages(marketing.id)).items.length).toBeGreaterThan(0)
  expect(await stateOf(client, marketing.id)).toBeUndefined()
  await expect(send(client, marketing.id, 'hi')).rejects.toMatchObject({ code: 'forbidden' })
  expect((await client.joinChannel(marketing.id)).isMember).toBe(true)
  expect(await stateOf(client, marketing.id)).toMatchObject({ unreadCount: 0, notify: 'mentions' })
})

test('seeded counts: mentions-only channel, muted channel, DM, and threads on their own', async () => {
  const { client } = setup()
  const general = await channel(client, 'general')
  const engineering = await channel(client, 'engineering')
  const random = await channel(client, 'random')
  // #general has a thread whose reply mentions the user: that is the thread's mention, not the channel's.
  expect(await stateOf(client, general.id)).toMatchObject({ unreadCount: 3, mentionCount: 0, notify: 'mentions' })
  expect(await stateOf(client, engineering.id)).toMatchObject({ unreadCount: 3, mentionCount: 1 })
  // Muted: the unread `@channel` is not a mention.
  expect(await stateOf(client, random.id)).toMatchObject({ unreadCount: 2, mentionCount: 0, notify: 'muted' })
  expect(await client.setNotify(random.id, 'mentions')).toMatchObject({ unreadCount: 2, mentionCount: 1 })

  const dms = (await client.listConversations()).filter((conversation) => conversation.kind === 'dm')
  expect(dms.map((dm) => dm.memberIds.length).sort()).toEqual([2, 2, 2, 3])
  const first = dms.find((dm) => dm.memberIds.length === 2 && dm.memberIds.includes('u2'))!
  expect(await stateOf(client, first.id)).toMatchObject({ unreadCount: 2, notify: 'all' })

  const followed = await client.listFollowedThreads()
  expect(followed.map((thread) => [thread.root.authorId === 'u1', thread.state.unreadReplies, thread.state.mentionCount])).toEqual([
    [false, 1, 1],
    [true, 2, 0],
  ])
  // Three threads exist; the one between other members is not followed.
  expect((await client.listThreads(general.id)).length).toBe(3)
})

test('a reply in a thread counts on the thread only', async () => {
  const { client, events } = setup({ simulateActivity: true })
  const general = await channel(client, 'general')
  await client.markRead(general.id)
  const root = await send(client, general.id, 'question')
  events.length = 0
  await nextTimers()
  // The simulated answer to the root lands in the channel.
  expect(await stateOf(client, general.id)).toMatchObject({ unreadCount: 1 })
  expect(events.some((event) => event.type === 'typing' && event.conversationId === general.id)).toBe(true)

  await client.markRead(general.id)
  await send(client, general.id, 'my reply', { threadRootId: root.id })
  events.length = 0
  await nextTimers()
  const thread = await client.getThread(root.id)
  expect(thread.items.map((reply) => reply.authorId === 'u1')).toEqual([true, false])
  expect(thread.root).toMatchObject({ replyCount: 2, replyUserIds: ['u1', thread.items[1].authorId] })
  expect(thread.state).toMatchObject({ following: true, unreadReplies: 1 })
  expect(await stateOf(client, general.id)).toMatchObject({ unreadCount: 0, mentionCount: 0 })
  // The reply is not in the main list, and the events carry the new thread count.
  expect((await client.listMessages(general.id)).items.some((message) => message.threadRootId !== null)).toBe(false)
  expect(events.find((event) => event.type === 'thread.changed')).toMatchObject({ state: { rootId: root.id, unreadReplies: 1 } })
  expect(events.some((event) => event.type === 'typing' && event.threadRootId === root.id)).toBe(true)

  expect(await client.markThreadRead(root.id)).toMatchObject({ unreadReplies: 0 })
})

test('an "also in channel" reply shows in the main list; @channel does nothing in a thread', async () => {
  const { client } = setup()
  const general = await channel(client, 'general')
  const root = await send(client, general.id, 'root')
  const reply = await send(client, general.id, '<!channel> <!here> look', { threadRootId: root.id, alsoInChannel: true })
  expect(reply).toMatchObject({ alsoInChannel: true, mentions: { channel: false, here: false } })
  expect((await client.listMessages(general.id)).items.at(-1)?.id).toBe(reply.id)
  expect((await send(client, general.id, '<!channel> look')).mentions.channel).toBe(true)
})

test('threads auto-follow: a 1:1 DM thread, and never an unrelated one', async () => {
  const { client } = setup({ simulateActivity: true })
  const dm = (await client.listConversations()).find((item) => item.kind === 'dm' && item.memberIds.length === 2)!
  await send(client, dm.id, 'hello')
  await nextTimers()
  expect(await stateOf(client, dm.id)).toMatchObject({ notify: 'all' })
  const general = await channel(client, 'general')
  const unfollowed = (await client.listThreads(general.id)).find((root) => root.authorId !== 'u1' && !root.replyUserIds.includes('u1'))!
  const followedIds = (await client.listFollowedThreads()).map((thread) => thread.root.id)
  expect(followedIds).not.toContain(unfollowed.id)
  // Unfollowing sticks; following again starts from now, with nothing unread.
  const started = (await client.listFollowedThreads()).find((thread) => thread.root.authorId === 'u1')!
  expect(await client.setThreadFollow(started.root.id, false)).toMatchObject({ following: false, unreadReplies: 0 })
  expect(await client.setThreadFollow(started.root.id, true)).toMatchObject({ following: true, unreadReplies: 0 })
})

test('mark read, mark unread and mark all read with undo', async () => {
  const { client, events } = setup()
  const general = await channel(client, 'general')
  const all = await allMessages(client, general.id)
  expect(await client.markRead(general.id)).toMatchObject({ unreadCount: 0, lastReadMessageId: all.at(-1)!.id })
  expect(events.at(-1)).toMatchObject({ type: 'state.changed', state: { conversationId: general.id, unreadCount: 0 } })

  // The last three are from other members: marking the first of them unread brings all three back.
  expect(await client.markUnread(all.at(-3)!.id)).toMatchObject({ unreadCount: 3, lastReadMessageId: all.at(-4)!.id })

  const before = await client.listStates()
  const previous = await client.markAllRead()
  expect(previous.states.length).toBeGreaterThan(1)
  expect(previous.threads.length).toBe(2)
  expect((await client.listStates()).every((state) => state.unreadCount === 0)).toBe(true)
  expect((await client.listFollowedThreads()).every((thread) => thread.state.unreadReplies === 0)).toBe(true)
  await client.restoreRead(previous)
  expect(await client.listStates()).toEqual(before)
  expect((await client.listFollowedThreads()).filter((thread) => thread.state.unreadReplies > 0).length).toBe(2)
})

test('sending echoes the nonce, reads the conversation and is limited to 4000 characters', async () => {
  const { client, events } = setup()
  const general = await channel(client, 'general')
  const sent = await client.sendMessage({ conversationId: general.id, body: 'hello', nonce: 'n-1' })
  expect(events.find((event) => event.type === 'message.created')).toMatchObject({ message: { id: sent.id, nonce: 'n-1' } })
  expect(await stateOf(client, general.id)).toMatchObject({ unreadCount: 0, lastReadMessageId: sent.id })
  await expect(send(client, general.id, 'x'.repeat(4001))).rejects.toMatchObject({ code: 'too_long' })
  await expect(client.editMessage(sent.id, 'x'.repeat(4001))).rejects.toMatchObject({ code: 'too_long' })
  expect((await send(client, general.id, 'x'.repeat(4000))).body.length).toBe(4000)
})

test('openDm returns the existing DM for the same set of members', async () => {
  const { client, events } = setup()
  const dms = (await client.listConversations()).filter((conversation) => conversation.kind === 'dm')
  const withU2 = dms.find((dm) => dm.memberIds.length === 2 && dm.memberIds.includes('u2'))!
  const group = dms.find((dm) => dm.memberIds.length === 3)!
  expect((await client.openDm(['u2'])).id).toBe(withU2.id)
  expect((await client.openDm(['u3', 'u2', 'u1'])).id).toBe(group.id)
  expect(events).toEqual([])

  const created = await client.openDm(['u4', 'u2'])
  expect(dms.map((dm) => dm.id)).not.toContain(created.id)
  expect(created).toMatchObject({ kind: 'dm', isMember: true })
  expect((await client.openDm(['u2', 'u4'])).id).toBe(created.id)
  expect(await stateOf(client, created.id)).toMatchObject({ notify: 'all', unreadCount: 0 })
  await expect(client.openDm(['nobody'])).rejects.toMatchObject({ code: 'not_found' })
})

test('#general cannot be left, archived or made private, not even by the owner', async () => {
  const member = setup().client
  const owner = setup({ currentUserId: 'u2' }).client
  const general = await channel(owner, 'general')
  await expect(member.leaveChannel(general.id)).rejects.toMatchObject({ code: 'forbidden' })
  await expect(owner.leaveChannel(general.id)).rejects.toMatchObject({ code: 'forbidden' })
  await expect(owner.archiveChannel(general.id)).rejects.toMatchObject({ code: 'forbidden' })
  await expect(owner.updateChannel(general.id, { kind: 'private' })).rejects.toMatchObject({ code: 'forbidden' })
  await expect(owner.removeMember(general.id, 'u3')).rejects.toMatchObject({ code: 'forbidden' })
  expect((await owner.updateChannel(general.id, { topic: 'Hello' })).topic).toBe('Hello')
})

test('leaving a public channel keeps it readable; leaving a private one removes it', async () => {
  const { client, events } = setup()
  const design = await channel(client, 'design')
  const leadership = await channel(client, 'leadership')
  await client.leaveChannel(design.id)
  expect(await channel(client, 'design')).toMatchObject({ isMember: false })
  expect(await stateOf(client, design.id)).toBeUndefined()
  await client.leaveChannel(leadership.id)
  expect(events.at(-1)).toEqual({ type: 'conversation.removed', conversationId: leadership.id })
  expect((await client.listConversations()).map((conversation) => conversation.name)).not.toContain('leadership')
})

test('permissions: messages', async () => {
  const member = setup().client
  const owner = setup({ currentUserId: 'u2' }).client
  const general = await channel(member, 'general')
  const all = await allMessages(member, general.id)
  const others = all.find((message) => message.authorId === 'u3' && message.kind === 'message')!
  const own = all.find((message) => message.authorId === 'u1' && message.replyCount === 0)!
  await expect(member.editMessage(others.id, 'changed')).rejects.toMatchObject({ code: 'forbidden' })
  await expect(member.deleteMessage(others.id)).rejects.toMatchObject({ code: 'forbidden' })
  expect(await member.editMessage(own.id, 'changed')).toMatchObject({ body: 'changed' })
  await member.deleteMessage(own.id)
  expect((await allMessages(member, general.id)).some((message) => message.id === own.id)).toBe(false)

  // The owner may delete, but not edit, another member's message.
  await expect(owner.editMessage(others.id, 'changed')).rejects.toMatchObject({ code: 'forbidden' })
  await owner.deleteMessage(others.id)
  expect((await allMessages(owner, general.id)).some((message) => message.id === others.id)).toBe(false)
})

test('a deleted root with replies stays as a deleted row; deleting its last reply removes it', async () => {
  const { client, events } = setup()
  const general = await channel(client, 'general')
  const root = await send(client, general.id, 'root')
  const reply = await send(client, general.id, 'reply', { threadRootId: root.id })
  await client.deleteMessage(root.id)
  expect((await client.getThread(root.id)).root).toMatchObject({ deleted: true, body: '', replyCount: 1 })
  events.length = 0
  await client.deleteMessage(reply.id)
  expect(events.filter((event) => event.type === 'message.deleted').map((event) => event.messageId)).toEqual([reply.id, root.id])
  await expect(client.getThread(root.id)).rejects.toMatchObject({ code: 'not_found' })
})

test('permissions: channels and categories', async () => {
  const member = setup().client
  const owner = setup({ currentUserId: 'u2' }).client
  const design = await channel(member, 'design')
  // #design was created by u3: u1 is a plain member of it.
  await expect(member.updateChannel(design.id, { topic: 'x' })).rejects.toMatchObject({ code: 'forbidden' })
  await expect(member.archiveChannel(design.id)).rejects.toMatchObject({ code: 'forbidden' })
  await expect(member.removeMember(design.id, 'u3')).rejects.toMatchObject({ code: 'forbidden' })
  await expect(member.createCategory('Ops')).rejects.toMatchObject({ code: 'forbidden' })
  await expect(member.move({ conversationId: design.id }, 'up')).rejects.toMatchObject({ code: 'forbidden' })
  const category = (await member.listCategories())[0]
  await expect(member.renameCategory(category.id, 'x')).rejects.toMatchObject({ code: 'forbidden' })
  await expect(member.deleteCategory(category.id)).rejects.toMatchObject({ code: 'forbidden' })

  // A creator manages their own channel, and names are unique.
  const mine = await member.createChannel({ name: 'My Project', kind: 'private', memberIds: ['u3'] })
  expect(mine).toMatchObject({ name: 'my-project', kind: 'private', isMember: true, memberIds: ['u1', 'u3'] })
  expect((await member.addMembers(mine.id, ['u4'])).memberIds).toEqual(['u1', 'u3', 'u4'])
  expect((await member.removeMember(mine.id, 'u3')).memberIds).toEqual(['u1', 'u4'])
  await expect(member.createChannel({ name: 'general', kind: 'public' })).rejects.toMatchObject({ code: 'conflict' })
  await member.archiveChannel(mine.id)
  expect((await member.listConversations()).some((conversation) => conversation.id === mine.id)).toBe(false)
  await expect(send(member, mine.id, 'hi')).rejects.toMatchObject({ code: 'forbidden' })

  // The owner manages any channel they can see, and the categories.
  const ownersDesign = await channel(owner, 'design')
  expect((await owner.updateChannel(ownersDesign.id, { topic: 'x' })).topic).toBe('x')
  const ops = await owner.createCategory('Ops')
  expect((await owner.listCategories()).map((item) => item.name)).toEqual(['Product', 'Company', 'Ops'])
  await owner.move({ categoryId: ops.id }, 'up')
  expect((await owner.listCategories()).map((item) => item.name)).toEqual(['Product', 'Ops', 'Company'])
  await owner.deleteCategory(category.id)
  expect((await channel(owner, 'design')).categoryId).toBeNull()
})

test('reactions and pins report the changed message', async () => {
  const { client, events } = setup()
  const general = await channel(client, 'general')
  const message = (await client.listMessages(general.id)).items.at(-1)!
  expect((await client.toggleReaction(message.id, '👍')).reactions).toContainEqual({ emoji: '👍', userIds: ['u1'] })
  expect((await client.toggleReaction(message.id, '👍')).reactions.some((reaction) => reaction.emoji === '👍')).toBe(false)
  const pinsBefore = (await client.listPins(general.id)).length
  expect(pinsBefore).toBe(1)
  events.length = 0
  await client.setPinned(message.id, true)
  expect((await client.listPins(general.id)).map((pin) => pin.id)).toContain(message.id)
  expect(events.map((event) => event.type)).toContain('message.updated')
  expect(events.some((event) => event.type === 'message.created' && event.message.kind === 'pin')).toBe(true)
})

test('alone in the workspace: only the user’s own messages, nothing unread, nothing simulated', async () => {
  const { client, events } = setup({ members: [{ id: 'u1', role: 'Owner' }], simulateActivity: true })
  const conversations = await client.listConversations()
  expect(conversations.some((conversation) => conversation.kind === 'dm')).toBe(false)
  for (const conversation of conversations) {
    const authors = new Set((await allMessages(client, conversation.id)).map((message) => message.authorId))
    if (authors.size > 1 || (authors.size === 1 && !authors.has('u1'))) throw new Error(`other authors in #${conversation.name}`)
  }
  expect((await client.listStates()).every((state) => state.unreadCount === 0 && state.mentionCount === 0)).toBe(true)
  expect(await client.getPresence()).toEqual(['u1'])
  const general = await channel(client, 'general')
  await send(client, general.id, 'anyone?')
  events.length = 0
  await nextTimers()
  expect(events).toEqual([])
})

test('no simulated activity with latencyMs 0 unless the test asks for it', async () => {
  const { client, events } = setup()
  const general = await channel(client, 'general')
  await send(client, general.id, 'hello')
  events.length = 0
  await nextTimers()
  expect(events).toEqual([])
})

test('upload reports progress and can be cancelled', async () => {
  const { client } = setup()
  const progress: number[] = []
  const file = new File(['hello'], 'note.txt', { type: 'text/plain' })
  const attachment = await client.uploadAttachment(file, { onProgress: (fraction) => progress.push(fraction) })
  expect(attachment).toMatchObject({ fileName: 'note.txt', mimeType: 'text/plain', fileSize: 5 })
  expect(attachment.url.startsWith('blob:')).toBe(true)
  expect(progress).toEqual([0.25, 0.5, 0.75, 1])

  const controller = new AbortController()
  const cancelled = client.uploadAttachment(file, { signal: controller.signal })
  controller.abort()
  await expect(cancelled).rejects.toMatchObject({ code: 'upload_failed' })
})
