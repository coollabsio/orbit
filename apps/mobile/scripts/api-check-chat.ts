/**
 * Drives every chat call the app's chat screens make against a real Orbit server, with the same functions, then
 * archives and deletes what it made.
 * Run: bun apps/mobile/scripts/api-check-chat.ts [server] [email] [password]
 * Defaults: http://127.0.0.1:18080, test@example.com, password.
 */
import { buildMessageRows, conversationActions, conversationPermissions, emojiGroups, hasReaction, isChatAdmin, newNonce, searchEmoji, sortFollowedThreads, splitByRanges, unreadConversations } from '../src/data/chat'
import * as chat from '../src/data/chat-api'
import * as api from '../src/data/orbit'

const [server = 'http://127.0.0.1:18080', email = 'test@example.com', password = 'password'] = process.argv.slice(2)

function check(ok: boolean, message: string) {
  if (!ok) throw new Error(`FAILED: ${message}`)
  console.log(`ok   ${message}`)
}

let token: string | null = null
const client = api.createClient(server, () => token)
const signIn = await api.signIn(client, email, password)
check(signIn.kind === 'signed_in', `signed in as ${email} without an Origin header`)
if (signIn.kind !== 'signed_in') process.exit(1)
token = signIn.token
const me = signIn.user.id
const cleanup: (() => Promise<unknown>)[] = []
const stamp = Date.now().toString(36)

try {
  const ws = (await api.workspaces(client))[0]!.id
  const members = await api.members(client, ws)
  const admin = isChatAdmin(members.find((member) => member.user_id === me)?.role)
  const other = members.find((member) => member.user_id !== me && !member.suspended_at)
  check(true, `workspace ${ws}, ${members.length} members, chat admin: ${admin}, other member: ${other?.display_name ?? 'none'}`)

  // categories
  const category = (await chat.createCategory(client, ws, `API check ${stamp}`)).result
  cleanup.push(async () => {
    await chat.deleteCategory(client, ws, category.id)
    console.log('ok   deleted the category')
  })
  const renamed = await chat.renameCategory(client, ws, category.id, `API check ${stamp} renamed`)
  check(renamed.result.name.endsWith('renamed') && renamed.events.some((event) => event.type === 'categories.changed'), `category created and renamed: "${renamed.result.name}", event categories.changed`)

  // a private channel in it, with the other member
  const created = await chat.createChannel(client, ws, { name: `api-check-${stamp}`, topic: 'made by api-check-chat', kind: 'private', categoryId: category.id, memberIds: [] })
  const channel = created.result
  cleanup.push(async () => {
    await chat.archiveChannel(client, ws, channel.id)
    console.log(`ok   archived #${channel.name}`)
  })
  check(channel.kind === 'private' && channel.category_id === category.id && channel.member_ids.join() === me, `created #${channel.name} (private, in the category, only me)`)
  const permissions = conversationPermissions(channel, me, admin)
  check(permissions.canManage && permissions.canAddPeople && permissions.canRemovePeople, `the creator manages it: ${conversationActions('header', undefined, permissions).join(', ')}`)
  const updated = await chat.updateChannel(client, ws, channel.id, { topic: 'edited topic' })
  check(updated.result.topic === 'edited topic', 'edit channel: topic saved')

  if (other) {
    const added = await chat.addMembers(client, ws, channel.id, [other.user_id])
    check(added.result.member_ids.includes(other.user_id), `add people: ${other.display_name} is in it`)
    const removed = await chat.removeMember(client, ws, channel.id, other.user_id)
    check(!removed.result.member_ids.includes(other.user_id), `remove: ${other.display_name} is out again`)
  }

  // messages, reactions, edit, pin
  const word = `zebra${stamp}`
  const sent = await chat.send(client, ws, channel.id, { body: `hello ${word}`, nonce: newNonce() })
  check(sent.events.some((event) => event.type === 'message.created'), `sent a message (${sent.result.id}) with a message.created event`)
  const reacted = await chat.react(client, ws, sent.result.id, '👍', true)
  check(hasReaction(reacted.result, '👍', me), 'reaction 👍 added')
  const unreacted = await chat.react(client, ws, sent.result.id, '👍', false)
  check(!hasReaction(unreacted.result, '👍', me) && unreacted.result.reactions.length === 0, 'reaction 👍 taken back')
  const edited = await chat.editMessage(client, ws, sent.result.id, `hello ${word} (changed)`)
  check(edited.result.edited_at !== null && edited.result.body.endsWith('(changed)'), 'edited the message')
  const pinned = await chat.setPinned(client, ws, sent.result.id, true)
  check(pinned.result.pinned && (await chat.pins(client, ws, channel.id)).some((item) => item.id === sent.result.id), 'pinned it; it is in the pins')
  const quoted = await chat.send(client, ws, channel.id, { body: 'a reply that quotes', nonce: newNonce(), replyToId: sent.result.id })
  check(quoted.result.reply_to?.id === sent.result.id, 'inline reply quotes the message')

  // threads
  const reply = await chat.send(client, ws, channel.id, { body: 'in the thread', nonce: newNonce(), threadRootId: sent.result.id, alsoInChannel: true })
  check(reply.result.thread_root_id === sent.result.id && reply.result.also_in_channel, 'thread reply, also sent to the channel')
  const thread = await chat.thread(client, ws, sent.result.id)
  check(thread.root.reply_count === 1 && thread.items.map((item) => item.id).join() === reply.result.id, `thread page: root with 1 reply, rows ${buildMessageRows(thread.items).length}`)
  const roots = await chat.conversationThreads(client, ws, channel.id)
  check(roots.some((item) => item.id === sent.result.id), 'the conversation lists the thread')
  const following = await chat.followThread(client, ws, sent.result.id, true)
  check(following.result.following, 'follow thread')
  const followed = sortFollowedThreads(await chat.followedThreads(client, ws))
  check(followed.some((item) => item.root.id === sent.result.id), `followed threads: ${followed.length}`)
  const readThread = await chat.markThreadRead(client, ws, sent.result.id)
  check(readThread.result.unread_replies === 0, 'thread read')
  const unfollowed = await chat.followThread(client, ws, sent.result.id, false)
  check(!unfollowed.result.following, 'unfollow thread')

  // a file
  const png = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg=='), (c) => c.charCodeAt(0))
  const file = await chat.uploadFile(client, ws, { part: new File([png], 'dot.png', { type: 'image/png' }), width: 1, height: 1 })
  check(file.mime_type === 'image/png' && file.width === 1, `uploaded ${file.file_name} (${file.size_bytes} B) as ${file.id}`)
  const withFile = await chat.send(client, ws, channel.id, { body: '', nonce: newNonce(), fileIds: [file.id] })
  check(withFile.result.attachments[0]?.id === file.id, 'a message with only the file')
  check((await chat.fileMessages(client, ws, channel.id)).some((item) => item.id === withFile.result.id), 'the files list has it')
  const download = await fetch(chat.serverUrl(server, file.url), { headers: { Authorization: `Bearer ${token}` } })
  check(download.ok && (await download.arrayBuffer()).byteLength === png.byteLength, `download with the bearer token: ${download.status}`)

  // search
  const found = await chat.search(client, ws, { query: word, conversationId: channel.id })
  const hit = found.items.find((item) => item.message.id === sent.result.id)
  check(hit !== undefined, `search "${word}": ${found.items.length} hit(s)`)
  check(splitByRanges(hit!.message.body, hit!.ranges).some((segment) => segment.match && segment.text.toLowerCase().includes('zebra')), 'the hit marks the word')
  const filtered = await chat.search(client, ws, { query: '', conversationId: channel.id, hasFile: true })
  check(filtered.items.some((item) => item.message.id === withFile.result.id), 'search "has file" finds the file message')
  const around = await chat.messagePage(client, ws, channel.id, { around: sent.result.id })
  check(around.items.some((item) => item.id === sent.result.id), 'messages around a search result')

  // read state, unread, favorite, notify
  const unread = await chat.markUnread(client, ws, withFile.result.id)
  check(unread.events.some((event) => event.type === 'state.changed'), `mark as unread: last read ${unread.result.last_read_message_id}`)
  const starred = await chat.setConversationState(client, ws, channel.id, { favorite: true, notify: 'muted' })
  check(starred.result.favorite && starred.result.notify === 'muted', 'star and mute')
  const states = await api.chatStates(client, ws)
  check(!unreadConversations(await api.conversations(client, ws), states, me).some((group) => group.conversation.id === channel.id), 'a muted conversation stays out of Unreads')
  await chat.setConversationState(client, ws, channel.id, { favorite: false, notify: 'all' })
  const read = await chat.markRead(client, ws, channel.id)
  check(read.result.unread_count === 0, 'mark as read')

  // delete what was written
  for (const message of [withFile.result, quoted.result, reply.result]) await chat.deleteMessage(client, ws, message.id)
  const deleted = await chat.deleteMessage(client, ws, sent.result.id)
  check(deleted.events.some((event) => event.type === 'message.deleted' || event.type === 'message.updated'), 'deleted the messages')

  // DMs and emoji
  const notes = await chat.openDm(client, ws, [])
  check(notes.result.kind === 'dm' && notes.result.self_dm, 'opened the DM with yourself')
  const emoji = await chat.customEmoji(client, ws)
  const groups = emojiGroups(await chat.emojibase(server))
  check(groups.length >= 8 && searchEmoji(groups, 'joy')[0]?.emoji === '😂', `emoji: ${groups.length} groups, ${groups.reduce((n, group) => n + group.emojis.length, 0)} emoji from the server, ${emoji.length} custom`)

  // leave: a second private channel with the other member. It is archived first, as a member who left could not archive it.
  if (other) {
    const extra = (await chat.createChannel(client, ws, { name: `api-check-leave-${stamp}`, topic: '', kind: 'private', categoryId: null, memberIds: [other.user_id] })).result
    await chat.archiveChannel(client, ws, extra.id)
    const left = await chat.leaveChannel(client, ws, extra.id)
    const after = (await api.conversations(client, ws)).find((item) => item.id === extra.id)
    check(left.result && !(after?.member_ids.includes(me) ?? false), `left #${extra.name} (archived first)`)
  }
} finally {
  for (const step of cleanup.reverse()) await step().catch((error: unknown) => console.log(`warn cleanup: ${error instanceof Error ? error.message : String(error)}`))
  await api.signOut(client)
  token = null
  console.log('ok   signed out')
}
