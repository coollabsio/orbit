#!/usr/bin/env bun
// Fills the development workspace with realistic demo tasks (for screenshots) through the HTTP API.
// Safe to run again: projects, labels and tasks that already exist (matched by name/title) are skipped.
//
//   bun scripts/seed-demo-tasks.ts                      # API at http://127.0.0.1:${ORBIT_DEV_API_PORT:-8080}
//   ORBIT_URL=http://127.0.0.1:18080 bun scripts/seed-demo-tasks.ts

const BASE = process.env.ORBIT_URL ?? `http://127.0.0.1:${process.env.ORBIT_DEV_API_PORT ?? 8080}`
// Development seed accounts (see `orbit seed`).
const LOGINS = { dev: 'test@example.com', member: 'member@example.com' }
const PASSWORD = 'password'
const WORKSPACE = process.env.ORBIT_WORKSPACE ?? 'Orbit Development'

type Status = 'Backlog' | 'Todo' | 'In Progress' | 'Done' | 'Cancelled' | 'Duplicate'
type Who = 'dev' | 'member'
type DemoTask = {
  project: string
  title: string
  status: Status
  priority: 'none' | 'low' | 'medium' | 'high' | 'urgent'
  labels: string[]
  assignees?: Who[]
  /** Days from today. */
  due?: number
  /** Days from today; turns the due date into a range for the roadmap. */
  start?: number
  description?: string
  comments?: [Who, string][]
  blocks?: string[]
  related?: string[]
  duplicateOf?: string
}

const PROJECTS = [
  { name: 'Orbit', key: 'ORB', color: '#e5488a' },
  { name: 'Coolify', key: 'COOLI', color: '#8b5cf6' },
  { name: 'Jean', key: 'GEN', color: '#5e6ad2' },
]

const LABELS = [
  { name: 'Bug', color: '#eb5757' },
  { name: 'Feature', color: '#5e6ad2' },
  { name: 'Improvement', color: '#4cb782' },
  { name: 'Design', color: '#bb87fc' },
  { name: 'Performance', color: '#f2994a' },
  { name: 'Security', color: '#d6409f' },
  { name: 'Docs', color: '#26b5ce' },
  { name: 'Chore', color: '#8b8f98' },
]

const TASKS: DemoTask[] = [
  // Orbit
  { project: 'Orbit', title: 'Kanban board with drag-and-drop between statuses', status: 'Done', priority: 'high', labels: ['Feature', 'Design'], assignees: ['dev'], due: -12,
    description: 'Cards move between columns and keep their order. Drop indicator shows where the card lands.',
    comments: [['member', 'Feels super smooth now, even with 200+ cards in a column.'], ['dev', 'Shipped 🚀']] },
  { project: 'Orbit', title: 'Roadmap timeline with drag-to-schedule', status: 'Done', priority: 'high', labels: ['Feature'], assignees: ['dev'], start: -20, due: -6 },
  { project: 'Orbit', title: 'Command palette: search tasks, projects and actions', status: 'Done', priority: 'medium', labels: ['Feature'], assignees: ['dev'], due: -9 },
  { project: 'Orbit', title: 'Bulk selection action bar', status: 'Done', priority: 'medium', labels: ['Improvement', 'Design'], assignees: ['member'], due: -4 },
  { project: 'Orbit', title: 'Realtime updates when a teammate edits a task', status: 'In Progress', priority: 'urgent', labels: ['Feature'], assignees: ['dev', 'member'], start: -3, due: 4,
    description: 'Push task changes over the realtime channel so open boards update without a refresh.\n\n- [x] Server events\n- [x] Query invalidation\n- [ ] Presence avatars',
    comments: [['member', 'Should we debounce invalidations when 20 events arrive at once?'], ['dev', 'Yes — batching them per animation frame.']] },
  { project: 'Orbit', title: 'Task list lags when scrolling 5,000 rows', status: 'In Progress', priority: 'high', labels: ['Bug', 'Performance'], assignees: ['dev'], due: 2,
    description: 'Virtualize the list and stop re-rendering every row on hover.' },
  { project: 'Orbit', title: 'My week view', status: 'Done', priority: 'medium', labels: ['Feature'], assignees: ['dev'], due: -2 },
  { project: 'Orbit', title: 'Keyboard shortcuts for status, priority and assignee', status: 'Todo', priority: 'medium', labels: ['Improvement'], assignees: ['member'], due: 6 },
  { project: 'Orbit', title: 'Recurring tasks', status: 'Todo', priority: 'low', labels: ['Feature'], start: 8, due: 18 },
  { project: 'Orbit', title: 'Mention notifications arrive twice', status: 'Todo', priority: 'high', labels: ['Bug'], assignees: ['dev'], due: 1,
    description: 'Steps: mention a teammate in a comment, then edit the comment. The inbox shows two notifications.' },
  { project: 'Orbit', title: 'Rate-limit login attempts per IP', status: 'Done', priority: 'urgent', labels: ['Security'], assignees: ['dev'], due: -15 },
  { project: 'Orbit', title: 'Import issues from GitHub', status: 'In Progress', priority: 'medium', labels: ['Feature'], assignees: ['member'], start: -5, due: 10 },
  { project: 'Orbit', title: 'Import issues from Linear', status: 'Duplicate', priority: 'low', labels: ['Feature'], duplicateOf: 'Import issues from GitHub' },
  { project: 'Orbit', title: 'Dark mode contrast on status badges', status: 'Backlog', priority: 'low', labels: ['Design'] },
  { project: 'Orbit', title: 'Write self-hosting guide', status: 'Backlog', priority: 'medium', labels: ['Docs'] },
  { project: 'Orbit', title: 'Mobile layout for the task detail panel', status: 'Backlog', priority: 'medium', labels: ['Design', 'Improvement'], start: 14, due: 28 },
  { project: 'Orbit', title: 'Offline mode', status: 'Cancelled', priority: 'low', labels: ['Feature'],
    description: 'Out of scope for v1. Revisit after realtime ships.' },
  { project: 'Orbit', title: 'Nightly SQLite backups with retention', status: 'Todo', priority: 'high', labels: ['Chore', 'Security'], assignees: ['dev'], due: 5 },
  { project: 'Orbit', title: 'Public API tokens scoped per project', status: 'Done', priority: 'medium', labels: ['Feature', 'Security'], assignees: ['dev'], due: -25 },

  // Coolify
  { project: 'Coolify', title: 'Wildcard domains with Traefik HostRegexp and DNS-01', status: 'In Progress', priority: 'high', labels: ['Feature'], assignees: ['dev'], start: -7, due: 7,
    comments: [['member', 'Cloudflare DNS-01 works in my test. Hetzner DNS still fails.']] },
  { project: 'Coolify', title: 'Deployments stuck in "queued" after server restart', status: 'Todo', priority: 'urgent', labels: ['Bug'], assignees: ['dev', 'member'], due: 0,
    description: 'After a reboot the queue worker does not pick up pending deployments. Needs a recovery job at startup.' },
  { project: 'Coolify', title: 'Build logs stream slowly on large Docker builds', status: 'Backlog', priority: 'medium', labels: ['Performance'] },
  { project: 'Coolify', title: 'One-click Supabase template', status: 'Done', priority: 'medium', labels: ['Feature'], assignees: ['member'], due: -10 },
  { project: 'Coolify', title: 'Encrypt environment variables at rest', status: 'Done', priority: 'high', labels: ['Security'], assignees: ['dev'], due: -18 },
  { project: 'Coolify', title: 'Preview deployments for pull requests', status: 'In Progress', priority: 'high', labels: ['Feature'], assignees: ['member'], start: -2, due: 12 },
  { project: 'Coolify', title: 'Server metrics dashboard redesign', status: 'Todo', priority: 'medium', labels: ['Design'], assignees: ['member'], start: 3, due: 15 },
  { project: 'Coolify', title: 'Update API reference for v4 endpoints', status: 'Todo', priority: 'low', labels: ['Docs'], due: 9 },
  { project: 'Coolify', title: 'Health check ignores custom port', status: 'Done', priority: 'high', labels: ['Bug'], assignees: ['dev'], due: -3 },
  { project: 'Coolify', title: 'Upgrade Laravel to 12', status: 'Backlog', priority: 'low', labels: ['Chore'] },
  { project: 'Coolify', title: 'S3 backup restore fails for Postgres 17', status: 'Todo', priority: 'high', labels: ['Bug'], assignees: ['dev'], due: 3 },
  { project: 'Coolify', title: 'Support Podman as container runtime', status: 'Cancelled', priority: 'none', labels: ['Feature'] },

  // Jean
  { project: 'Jean', title: 'Launch WSL terminal CLIs in exec mode', status: 'In Progress', priority: 'medium', labels: ['Bug'], assignees: ['dev'], due: 2 },
  { project: 'Jean', title: 'Parallel sessions per worktree', status: 'Done', priority: 'high', labels: ['Feature'], assignees: ['dev'], due: -8 },
  { project: 'Jean', title: 'Run environments panel shows stale ports', status: 'Todo', priority: 'medium', labels: ['Bug'], assignees: ['member'], due: 4 },
  { project: 'Jean', title: 'Faster startup: lazy-load project list', status: 'Backlog', priority: 'medium', labels: ['Performance'], start: 10, due: 20 },
  { project: 'Jean', title: 'Onboarding tour for first launch', status: 'Backlog', priority: 'low', labels: ['Design', 'Docs'] },
  { project: 'Jean', title: 'Code review summary in the PR panel', status: 'Todo', priority: 'high', labels: ['Feature'], assignees: ['dev'], start: 2, due: 11 },
]

const RELATIONS: { from: string; to: string; type: 'blocks' | 'related' }[] = [
  { from: 'Realtime updates when a teammate edits a task', to: 'Mention notifications arrive twice', type: 'related' },
  { from: 'Task list lags when scrolling 5,000 rows', to: 'Keyboard shortcuts for status, priority and assignee', type: 'blocks' },
  { from: 'Deployments stuck in "queued" after server restart', to: 'Preview deployments for pull requests', type: 'blocks' },
  { from: 'Nightly SQLite backups with retention', to: 'Write self-hosting guide', type: 'blocks' },
  { from: 'Wildcard domains with Traefik HostRegexp and DNS-01', to: 'Preview deployments for pull requests', type: 'related' },
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

async function all<T>(path: string): Promise<T[]> {
  const items: T[] = []
  let cursor: string | null | undefined
  do {
    const separator = path.includes('?') ? '&' : '?'
    const page = await api<{ items: T[]; next_cursor?: string | null }>('GET', `${path}${separator}limit=100${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''}`)
    items.push(...page.items)
    cursor = page.next_cursor
  } while (cursor)
  return items
}

const day = (offset: number) => {
  const date = new Date()
  date.setUTCHours(17, 0, 0, 0)
  date.setUTCDate(date.getUTCDate() + offset)
  return date.toISOString()
}

const cookies = {} as Record<Who, string>
const users = {} as Record<Who, string[]>
for (const who of ['member', 'dev'] as const) {
  cookie = ''
  const login = await api<{ user: { id: string } }>('POST', '/api/v1/auth/login', { email: LOGINS[who], password: PASSWORD })
  cookies[who] = cookie
  users[who] = [login.user.id]
}
const workspaces = await api<{ id: string; name: string }[]>('GET', '/api/v1/workspaces')
const workspace = workspaces.find((item) => item.name === WORKSPACE) ?? workspaces[0]
const ws = `/api/v1/workspaces/${workspace.id}`
console.log(`workspace: ${workspace.name}`)

const projects = new Map((await all<{ id: string; name: string }>(`${ws}/projects`)).map((item) => [item.name, item.id]))
for (const project of PROJECTS) {
  if (projects.has(project.name)) continue
  const created = await api<{ id: string }>('POST', `${ws}/projects`, project)
  projects.set(project.name, created.id)
  console.log(`+ project ${project.name}`)
}

const labels = new Map((await all<{ id: string; name: string }>(`${ws}/labels`)).map((item) => [item.name, item.id]))
for (const label of LABELS) {
  if (labels.has(label.name)) continue
  const created = await api<{ id: string }>('POST', `${ws}/labels`, label)
  labels.set(label.name, created.id)
  console.log(`+ label ${label.name}`)
}

const statuses = new Map<string, Map<string, string>>()
for (const [name, id] of projects) {
  const list = await all<{ id: string; name: string }>(`${ws}/projects/${id}/statuses`)
  statuses.set(name, new Map(list.map((item) => [item.name, item.id])))
}

const existing = new Map((await all<{ id: string; title: string }>(`${ws}/tasks`)).map((item) => [item.title, item.id]))
const created = new Map<string, { id: string; version: number }>()
for (const task of TASKS) {
  if (existing.has(task.title)) continue
  const projectStatuses = statuses.get(task.project)!
  // Duplicates start in Backlog; marking them below moves them to the Duplicate status.
  const statusName = task.status === 'Duplicate' ? 'Backlog' : task.status
  const record = await api<{ id: string; version: number }>('POST', `${ws}/tasks`, {
    project_id: projects.get(task.project),
    status_id: projectStatuses.get(statusName),
    title: task.title,
    description: task.description ?? '',
    priority: task.priority,
    label_ids: task.labels.map((name) => labels.get(name)),
    assignee_ids: (task.assignees ?? []).flatMap((who) => users[who]),
    due_start_at: task.start === undefined ? null : day(task.start),
    due_at: task.due === undefined ? null : day(task.due),
  })
  created.set(task.title, record)
  existing.set(task.title, record.id)
  console.log(`+ task ${task.title}`)
}

for (const task of TASKS) {
  const record = created.get(task.title)
  if (!record) continue
  if (task.duplicateOf) {
    await api('PATCH', `${ws}/tasks/${record.id}`, { expected_version: record.version, duplicate_of_id: existing.get(task.duplicateOf) })
  }
  for (const [who, body] of task.comments ?? []) {
    cookie = cookies[who]
    await api('POST', `${ws}/tasks/${record.id}/comments`, { body })
  }
  cookie = cookies.dev
}

for (const relation of RELATIONS) {
  if (!created.has(relation.from) && !created.has(relation.to)) continue
  await api('POST', `${ws}/tasks/${existing.get(relation.from)}/relations`, { task_id: existing.get(relation.to), type: relation.type })
}

console.log(`done: ${created.size} new task(s)`)
