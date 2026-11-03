import { createOrbitClient } from '@orbit/api/client'
import { expect, test } from 'bun:test'

import * as chat from './chat-api'

interface Sent {
  method: string
  url: string
  body: string
  form: FormData | null
}

/** A client whose fetch records the request and answers `reply`. */
function recording(reply: unknown, status = 200) {
  const sent: Sent[] = []
  const client = createOrbitClient({
    baseUrl: 'https://orbit.test',
    token: () => 'secret',
    fetch: async (request) => {
      const multipart = request.headers.get('content-type')?.startsWith('multipart/form-data') ?? false
      const form = multipart ? await request.formData() : null
      sent.push({ method: request.method, url: request.url, body: form ? '' : await request.text(), form })
      return new Response(JSON.stringify(reply), { status, headers: { 'content-type': 'application/json' } })
    },
  })
  return { client, sent }
}

test('a thread reply that also goes to the channel sends every field of the web composer', async () => {
  const { client, sent } = recording({ result: { id: 'm9' }, events: [{ type: 'emoji.changed' }], seq: 4 })
  const written = await chat.send(client, 'w1', 'c1', { body: 'hi', nonce: 'n1', threadRootId: 'r1', fileIds: ['f1'], alsoInChannel: true })
  expect(written).toEqual({ result: { id: 'm9' } as never, events: [{ type: 'emoji.changed' }] })
  expect(sent.map(({ method, url, body }) => [method, url, JSON.parse(body)])).toEqual([
    [
      'POST',
      'https://orbit.test/api/v1/workspaces/w1/chat/conversations/c1/messages',
      { body: 'hi', nonce: 'n1', thread_root_id: 'r1', reply_to_id: null, file_ids: ['f1'], also_in_channel: true },
    ],
  ])
})

test('reactions go on with PUT and off with DELETE, the emoji encoded in the path', async () => {
  const { client, sent } = recording({ result: { id: 'm1' }, events: [], seq: 1 })
  await chat.react(client, 'w1', 'm1', '👍', true)
  await chat.react(client, 'w1', 'm1', ':party_parrot:', false)
  expect(sent.map(({ method, url }) => `${method} ${url}`)).toEqual([
    'PUT https://orbit.test/api/v1/workspaces/w1/chat/messages/m1/reactions/%F0%9F%91%8D',
    'DELETE https://orbit.test/api/v1/workspaces/w1/chat/messages/m1/reactions/%3Aparty_parrot%3A',
  ])
})

test('an upload is one multipart file part, with the image size in the query', async () => {
  const { client, sent } = recording({ id: 'f1', file_name: 'a.png' }, 201)
  const file = await chat.uploadFile(client, 'w1', { part: new File(['png'], 'a.png', { type: 'image/png' }), width: 40, height: 30 })
  expect(file).toEqual({ id: 'f1', file_name: 'a.png' } as never)
  expect(sent[0]!.url).toBe('https://orbit.test/api/v1/workspaces/w1/chat/files?width=40&height=30')
  const part = sent[0]!.form!.get('file') as File
  expect([part.name, part.type, await part.text()]).toEqual(['a.png', 'image/png', 'png'])
})

test('search leaves out empty filters', async () => {
  const { client, sent } = recording({ items: [], cursor: null })
  await chat.search(client, 'w1', { query: 'deploy', conversationId: 'c1' })
  await chat.search(client, 'w1', { query: '', authorId: 'u1', hasFile: true })
  expect(sent.map(({ url }) => url)).toEqual([
    'https://orbit.test/api/v1/workspaces/w1/chat/search?query=deploy&conversation_id=c1',
    'https://orbit.test/api/v1/workspaces/w1/chat/search?author_id=u1&has_file=true',
  ])
})

test('a new private channel sends its people and category', async () => {
  const { client, sent } = recording({ result: { id: 'c2' }, events: [], seq: 1 })
  await chat.createChannel(client, 'w1', { name: 'design', topic: '', kind: 'private', categoryId: 'k1', memberIds: ['u2'] })
  expect(JSON.parse(sent[0]!.body)).toEqual({ name: 'design', topic: '', kind: 'private', category_id: 'k1', member_ids: ['u2'] })
})

test('server paths become absolute; absolute addresses stay', () => {
  expect(chat.serverUrl('https://orbit.test/', '/api/v1/x')).toBe('https://orbit.test/api/v1/x')
  expect(chat.serverUrl('https://orbit.test', 'https://cdn.test/a.png')).toBe('https://cdn.test/a.png')
})
