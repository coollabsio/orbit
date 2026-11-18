// Fills an empty Orbit server with the "Acme" demo workspace for App Store screenshots.
// Usage: node seed-demo.mjs <base URL> <setup token>   (a fresh server: run it once)
// Writes the owner's session token to ./maya.token and prints the IDs that shoot.mjs needs.
import { writeFileSync } from 'node:fs'

const [B, SETUP] = process.argv.slice(2)
const PASSWORD = 'demo-password-1'
const json = { 'content-type': 'application/json' }

async function call(token, method, path, body, extra = {}) {
  const res = await fetch(B + path, {
    method,
    headers: { ...json, ...(token ? { authorization: `Bearer ${token}` } : {}), ...extra },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const text = await res.text()
  if (!res.ok) throw new Error(`${method} ${path} ${res.status}: ${text.slice(0, 300)}`)
  return text ? JSON.parse(text) : null
}
const login = async (email) => (await call(null, 'POST', '/api/v1/auth/login', { email, password: PASSWORD })).session_token
const day = (offset, hour = 17) => {
  const d = new Date()
  d.setDate(d.getDate() + offset)
  d.setHours(hour, 0, 0, 0)
  return d.toISOString()
}

// owner and workspace
await call(null, 'POST', '/api/v1/setup/complete', { token: SETUP, email: 'maya@acme.example', display_name: 'Maya Chen', password: PASSWORD }, { origin: B })
const maya = await login('maya@acme.example')
writeFileSync('maya.token', maya)
const ws = (await call(maya, 'POST', '/api/v1/workspaces', { name: 'Acme' })).id
const W = `/api/v1/workspaces/${ws}`

// team
const people = { maya: { token: maya } }
for (const [key, name] of [['leo', 'Leo Park'], ['sara', 'Sara Novak'], ['jon', 'Jon Ruiz'], ['ana', 'Ana Ito']]) {
  const email = `${key}@acme.example`
  const { url } = await call(maya, 'POST', `${W}/invitations`, { email, role: 'member', delivery: 'manual' })
  const token = new URL(url).searchParams.get('token')
  await call(null, 'POST', '/api/v1/workspaces/invitations/accept', { token, email, display_name: name, password: PASSWORD }, { origin: B })
  people[key] = { token: await login(email) }
}
const members = await call(maya, 'GET', `${W}/members`)
const list = members.members ?? members.items ?? members
for (const [key, p] of Object.entries(people)) p.id = list.find((m) => (m.email ?? m.user?.email) === `${key}@acme.example`)?.user_id ?? list.find((m) => (m.email ?? m.user?.email) === `${key}@acme.example`)?.user?.id
const id = (key) => people[key].id

// projects and tasks
const projects = {}
for (const [key, name, code, color] of [['mobile', 'Mobile App', 'MOB', '#e5484d'], ['web', 'Website', 'WEB', '#3e63dd'], ['launch', 'Launch', 'LCH', '#30a46c']]) {
  const project = await call(maya, 'POST', `${W}/projects`, { name, key: code, color })
  const statuses = await call(maya, 'GET', `${W}/projects/${project.id}/statuses`)
  const all = statuses.statuses ?? statuses.items ?? statuses
  const by = (re) => all.find((s) => re.test(s.name))?.id ?? all[0].id
  projects[key] = { id: project.id, todo: by(/^todo|to do/i), progress: by(/progress/i), done: by(/done/i), backlog: by(/backlog/i) }
}
const tasks = [
  ['mobile', 'Ship push notifications to TestFlight', 'progress', 'urgent', ['maya'], 0, 'Production APNs key is set. Check the payload on iOS 18 and iOS 26.'],
  ['mobile', 'Offline mode for task lists', 'progress', 'high', ['leo'], 2, 'Cache the last 200 tasks per project and sync changes when the network comes back.'],
  ['mobile', 'Polish the iPad sidebar', 'todo', 'medium', ['maya'], 1, ''],
  ['mobile', 'Fix keyboard covering the composer', 'todo', 'high', ['sara'], 3, ''],
  ['mobile', 'App Store screenshots and listing', 'todo', 'medium', ['maya', 'ana'], 4, ''],
  ['mobile', 'Haptics on swipe actions', 'backlog', 'low', ['leo'], null, ''],
  ['web', 'New pricing page', 'progress', 'high', ['ana'], 1, ''],
  ['web', 'Customer stories section', 'todo', 'medium', ['jon'], 6, ''],
  ['web', 'Speed up the docs search', 'done', 'medium', ['sara'], -2, ''],
  ['web', 'Dark mode for the blog', 'backlog', 'low', ['ana'], null, ''],
  ['launch', 'Write the launch announcement', 'progress', 'high', ['maya'], 2, ''],
  ['launch', 'Record the product demo video', 'todo', 'medium', ['jon'], 5, ''],
  ['launch', 'Prepare the press kit', 'todo', 'medium', ['ana'], 6, ''],
  ['launch', 'Invite beta users to the launch call', 'done', 'low', ['jon'], -1, ''],
]
const created = []
for (const [project, title, status, priority, who, due, description] of tasks) {
  const p = projects[project]
  const task = await call(maya, 'POST', `${W}/tasks`, {
    project_id: p.id,
    status_id: p[status],
    title,
    priority,
    assignee_ids: who.map(id),
    due_at: due === null ? null : day(due),
    ...(description ? { description } : {}),
  })
  created.push(task)
}
const hero = created[0]
await call(people.leo.token, 'POST', `${W}/tasks/${hero.id}/comments`, { body: `@Maya Chen pushes now arrive in under a second on TestFlight.`, mentioned_user_ids: [id('maya')] })
await call(people.sara.token, 'POST', `${W}/tasks/${hero.id}/comments`, { body: 'Tested on my iPhone 16: tapping a notification opens the right chat. 🎉' })
await call(people.ana.token, 'POST', `${W}/tasks/${created[4].id}/comments`, { body: '@Maya Chen I added the iPad sizes to the checklist.', mentioned_user_ids: [id('maya')] })

for (const [who, project, title, due] of [['leo', 'mobile', 'Review the offline sync PR', 1], ['sara', 'web', 'Approve the new app icon', 2], ['jon', 'launch', 'Confirm the launch date with partners', 3]]) {
  await call(people[who].token, 'POST', `${W}/tasks`, { project_id: projects[project].id, status_id: projects[project].todo, title, priority: 'medium', assignee_ids: [id('maya')], due_at: day(due) })
}

// chat
const product = (await call(maya, 'POST', `${W}/chat/conversations`, { name: 'product', kind: 'public', topic: 'Roadmap, releases and feedback', member_ids: ['leo', 'sara', 'jon', 'ana'].map(id) })).result
await call(maya, 'POST', `${W}/chat/conversations`, { name: 'design', kind: 'public', topic: 'Mockups and reviews', member_ids: ['sara', 'ana'].map(id) })
await call(maya, 'POST', `${W}/chat/conversations`, { name: 'launch', kind: 'public', topic: 'Launch week plan', member_ids: ['jon', 'ana'].map(id) })
const say = async (who, body, extra = {}) => (await call(people[who].token, 'POST', `${W}/chat/conversations/${product.id}/messages`, { body, nonce: crypto.randomUUID(), ...extra })).result
await say('sara', 'New onboarding mockups are in the Design principles doc. Feedback welcome!')
await say('jon', 'Looks great. The empty states are much friendlier now. 👏')
await say('ana', 'Agreed. Can we use the same illustrations on the website?')
await say('maya', 'Morning! The iOS build is in TestFlight. Please try it today and post anything odd here. 📱')
const root = await say('leo', 'Offline mode is almost done. Lists load instantly now, even in airplane mode.')
await say('sara', 'Nice! Does it also queue new comments?', { thread_root_id: root.id })
await say('leo', 'Yes, they send when the network comes back.', { thread_root_id: root.id })
await say('ana', 'Pricing page draft is ready for review: three plans, yearly toggle, FAQ at the bottom.')
await say('jon', '@Maya Chen can we move the demo recording to Thursday? The new onboarding lands Wednesday.')
await say('maya', 'Thursday works. I will block 2 hours in the morning. ✅')

// docs (public API, with an API token)
const apiToken = await call(maya, 'POST', `${W}/api-tokens`, { name: 'demo seed', scopes: ['docs:write'], all_projects: true })
const secret = apiToken.token
const doc = (title, icon, markdown, parent_id) => call(secret, 'POST', `/api/public/v1/workspaces/${ws}/docs`, { title, icon, markdown, parent_id })
const handbook = await doc('Team handbook', '📘', 'How we plan, ship and talk to each other.\n\n## Weekly rhythm\n\n- **Monday:** plan the week in Tasks\n- **Wednesday:** demo what shipped\n- **Friday:** write the changelog\n\n## Where things live\n\n| What | Where |\n| --- | --- |\n| Plans and bugs | Tasks |\n| Specs and notes | Docs |\n| Quick questions | Chat |\n')
const roadmap = await doc('Q1 roadmap', '🗺️', 'Our three goals for the quarter.\n\n## 1. Mobile apps\n\nShip the iOS app to the App Store, then Android.\n\n- [x] TestFlight build\n- [x] Push notifications\n- [ ] Offline mode\n- [ ] App Store release\n\n## 2. Faster onboarding\n\nA new workspace is useful in under two minutes: templates, sample tasks and a guided tour.\n\n## 3. Launch\n\n> Ship small, ship often, and tell people about it.\n\nPublic launch at the end of February, with a demo video and a press kit.\n')
await doc('Launch checklist', '🚀', '- [x] Pick the date\n- [x] Beta call with users\n- [ ] Announcement post\n- [ ] Demo video\n- [ ] Press kit\n')
await doc('Meeting notes', '🗒️', '**Attendees:** Maya, Leo, Sara, Jon, Ana\n\n- iOS build in TestFlight, feedback in #product\n- Pricing page review on Friday\n- Demo recording moved to Thursday\n')
await doc('Design principles', '🎨', '1. Fast beats fancy\n2. One clear action per screen\n3. Keyboard and touch are equal\n', handbook.id)

console.log(JSON.stringify({ ws, task: hero.id, chat: product.id, doc: roadmap.id }))
