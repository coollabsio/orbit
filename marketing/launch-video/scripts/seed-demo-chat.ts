#!/usr/bin/env bun
// Fills the development workspace with demo chat channels and messages (for screenshots) through the HTTP API.
// Channels that already exist (matched by name) are reused. Run it once: it adds messages every time it runs.
//
//   ORBIT_URL=http://127.0.0.1:18080 bun marketing/launch-video/scripts/seed-demo-chat.ts

const BASE = process.env.ORBIT_URL ?? `http://127.0.0.1:${process.env.ORBIT_DEV_API_PORT ?? 8080}`
// Development seed accounts (see `orbit seed`).
const LOGINS = { dev: 'test@example.com', member: 'member@example.com' }
const PASSWORD = 'password'
const WORKSPACE = process.env.ORBIT_WORKSPACE ?? 'Orbit Development'

type Who = keyof typeof LOGINS
type Line = { who: Who; body: string; reactions?: string[]; thread?: [Who, string][] }

const CHANNELS: { name: string; topic: string; lines: Line[] }[] = [
  {
    name: 'design',
    topic: 'Mockups, icons and UI reviews',
    lines: [
      { who: 'member', body: 'New empty states for Docs are up in the Figma file 🎨', reactions: ['😍'] },
      { who: 'dev', body: 'Love the pixel illustrations. Shipping them with the next release.' },
    ],
  },
  {
    name: 'release',
    topic: 'Release checklist and changelog',
    lines: [
      { who: 'dev', body: 'Changelog for **v1.0** is ready for review.' },
      { who: 'member', body: 'Looks good. I added the screenshots for the Notion import.', reactions: ['✅'] },
    ],
  },
  {
    name: 'engineering',
    topic: 'Backend, frontend and infrastructure',
    lines: [
      { who: 'dev', body: 'Morning! The realtime sync for task edits is merged 🎉', reactions: ['🎉', '🚀'] },
      {
        who: 'member',
        body: 'Nice. Boards update in under 100 ms now, even with 5,000 tasks.',
        thread: [
          ['dev', 'Did you test it on Postgres too?'],
          ['member', 'Yes, both SQLite and Postgres are green ✅'],
        ],
      },
      {
        who: 'dev',
        body: 'This is the handler that sends the event:\n\n```rust\nasync fn publish(state: &AppState, event: TaskEvent) {\n    state.realtime.send(event.workspace_id, event).await;\n}\n```',
      },
      { who: 'member', body: 'Clean! Can we use the same channel for docs presence?', reactions: ['👀'] },
      { who: 'dev', body: 'Yes. I will open a task for it and link the design doc.' },
      { who: 'member', body: 'The whole server still uses less memory than one browser tab 😄', reactions: ['🔥', '😂'] },
    ],
  },
]

let cookie = ''
async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(`${BASE}${path}`, {
    method,
    headers: { 'content-type': 'application/json', origin: BASE, 'x-orbit-contract': 'orbit-api-v1', cookie },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const setCookie = response.headers.get('set-cookie')
  if (setCookie) cookie = setCookie.split(';')[0]
  if (!response.ok) throw new Error(`${method} ${path} → ${response.status}: ${await response.text()}`)
  return (response.status === 204 ? undefined : await response.json()) as T
}

const cookies = {} as Record<Who, string>
const ids = {} as Record<Who, string>
for (const who of ['member', 'dev'] as const) {
  cookie = ''
  const login = await api<{ user: { id: string } }>('POST', '/api/v1/auth/login', { email: LOGINS[who], password: PASSWORD })
  cookies[who] = cookie
  ids[who] = login.user.id
}
const as = (who: Who) => {
  cookie = cookies[who]
}

as('dev')
const workspaces = await api<{ id: string; name: string }[]>('GET', '/api/v1/workspaces')
const workspace = workspaces.find((item) => item.name === WORKSPACE) ?? workspaces[0]
const ws = `/api/v1/workspaces/${workspace.id}/chat`

type Written<T> = { result: T }
const unwrap = <T>(written: Written<T>): T => written.result

as('dev')
const listed = await api<{ items?: { id: string; name: string | null }[] } | { id: string; name: string | null }[]>('GET', `${ws}/conversations`)
const existing = new Map((Array.isArray(listed) ? listed : (listed.items ?? [])).map((item) => [item.name, item.id]))

for (const channel of CHANNELS) {
  as('dev')
  const conversation = existing.has(channel.name)
    ? { id: existing.get(channel.name)! }
    : unwrap(await api<Written<{ id: string }>>('POST', `${ws}/conversations`, { name: channel.name, topic: channel.topic, kind: 'public', member_ids: [ids.member] }))
  for (const line of channel.lines) {
    as(line.who)
    const message = unwrap(await api<Written<{ id: string }>>('POST', `${ws}/conversations/${conversation.id}/messages`, { body: line.body, nonce: crypto.randomUUID() }))
    for (const [index, emoji] of (line.reactions ?? []).entries()) {
      as(index % 2 === 0 ? (line.who === 'dev' ? 'member' : 'dev') : line.who)
      await api('PUT', `${ws}/messages/${message.id}/reactions/${encodeURIComponent(emoji)}`)
    }
    for (const [who, body] of line.thread ?? []) {
      as(who)
      await api('POST', `${ws}/conversations/${conversation.id}/messages`, { body, thread_root_id: message.id, nonce: crypto.randomUUID() })
    }
  }
  console.log(`+ #${channel.name}`)
}
console.log('done')
