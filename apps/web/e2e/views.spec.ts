import { expect, test, type Page, type Route } from '@playwright/test'
import { testWorkspace } from '../src/test/workspace'

test.use({ viewport: { width: 1280, height: 800 } })

type Person = { id: string; display_name: string; email: string; role: 'owner' | 'member' }
const OWNER: Person = { id: 'user-1', display_name: 'Ada Owner', email: 'ada@orbit.test', role: 'owner' }
const MEMBER: Person = { id: 'user-2', display_name: 'Grace Member', email: 'grace@orbit.test', role: 'member' }
const PEOPLE = [OWNER, MEMBER]
const WS = '/api/v1/workspaces/alpha'

type StoredView = {
  id: string
  owner_user_id: string
  name: string
  description: string
  icon: string | null
  color: string | null
  visibility: 'personal' | 'workspace'
  state: unknown
  version: number
}
type Store = { views: StoredView[]; favorites: Map<string, string[]>; queries: Array<{ user: string; body: { filter?: unknown } }>; patches: unknown[] }

const newStore = (): Store => ({ views: [], favorites: new Map(), queries: [], patches: [] })

const DEFAULT_DISPLAY = {
  layout: 'list',
  group_by: 'status',
  sub_group_by: 'none',
  order_by: 'manual',
  order_direction: 'asc',
  properties: ['id', 'status', 'assignee', 'priority', 'project', 'due_date', 'labels'],
  show_completed: 'all',
  show_empty_groups: false,
}
const HIGH_STATE = { filter: { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['high'] }] }, display: DEFAULT_DISPLAY }

const PROJECT = { id: 'project-1', workspace_id: 'alpha', name: 'Launch', key: 'LAU', color: '#e0457b', version: 1, created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z' }
const STATUSES = [
  { id: 'todo', project_id: 'project-1', name: 'Todo', description: '', category: 'unstarted', color: '#8b8f98', position: 0, version: 1 },
  { id: 'done', project_id: 'project-1', name: 'Done', description: '', category: 'completed', color: '#4cb782', position: 1, version: 1 },
]
const task = (id: string, title: string, priority: string) => ({
  id,
  workspace_id: 'alpha',
  project_id: 'project-1',
  status_id: 'todo',
  title,
  description: '',
  position: 1,
  priority,
  assignee_ids: [],
  label_ids: [],
  creator_id: 'user-1',
  blocked: false,
  duplicate_of: null,
  due_at: null,
  created_at: '2026-09-01T12:00:00Z',
  updated_at: '2026-09-01T12:00:00Z',
  version: 1,
})
const TASKS = [task('task-1', 'Fix the login loop', 'high'), task('task-2', 'Polish the empty state', 'low')]

const visibleTo = (store: Store, person: Person) => store.views.filter((view) => view.visibility === 'workspace' || view.owner_user_id === person.id)

function record(view: StoredView, person: Person, store: Store) {
  const favorites = store.favorites.get(person.id) ?? []
  const owner = PEOPLE.find((candidate) => candidate.id === view.owner_user_id) ?? OWNER
  return {
    id: view.id,
    workspace_id: 'alpha',
    owner: { user_id: owner.id, display_name: owner.display_name },
    name: view.name,
    description: view.description,
    icon: view.icon,
    color: view.color,
    visibility: view.visibility,
    state: view.state,
    state_error: null,
    version: view.version,
    is_favorite: favorites.includes(view.id),
    favorite_position: favorites.includes(view.id) ? favorites.indexOf(view.id) : null,
    can_edit: view.owner_user_id === person.id || person.role !== 'member',
    can_change_visibility: view.owner_user_id === person.id,
    created_at: '2026-09-01T12:00:00Z',
    updated_at: '2026-09-01T12:00:00Z',
  }
}

const problem = (status: number, code: string, detail: string, instance: string) => ({ type: 'about:blank', title: 'Request failed', status, code, detail, instance, request_id: 'e2e' })

const lastQueryFilter = (store: Store, person: Person) => JSON.stringify(store.queries.filter((query) => query.user === person.id).at(-1)?.body.filter ?? null)

/** A small, stateful stand-in for the views API. Shared across contexts, so two users see the same views. */
async function mockApi(page: Page, person: Person, store: Store) {
  await page.route('**/api/v1/**', async (route: Route) => {
    const request = route.request()
    const method = request.method()
    const path = new URL(request.url()).pathname
    const json = (body: unknown, status = 200) => route.fulfill({ status, json: body })
    const empty = () => route.fulfill({ status: 204 })

    if (path.endsWith('/setup/status')) return json({ complete: true })
    if (path.endsWith('/auth/me')) return json({ id: person.id, display_name: person.display_name, email: person.email })
    if (path === '/api/v1/workspaces') return json([testWorkspace(person.role, { id: 'alpha', name: 'Alpha' })])
    if (path === `${WS}/members`) {
      return json({
        items: PEOPLE.map((p, index) => ({ id: `membership-${index}`, user_id: p.id, display_name: p.display_name, email: p.email, role: p.role, can_change_role: false, can_remove: false, can_transfer_ownership: false, version: 1, created_at: '2026-09-01T12:00:00Z' })),
        next_cursor: null,
      })
    }
    if (path === `${WS}/projects`) return json({ items: [PROJECT], next_cursor: null })
    if (path.endsWith('/statuses')) return json({ items: STATUSES, next_cursor: null })
    if (path === `${WS}/tasks/query` && method === 'POST') {
      store.queries.push({ user: person.id, body: request.postDataJSON() })
      return json({ items: TASKS, next_cursor: null })
    }
    if (path.startsWith(`${WS}/view-preferences/`)) {
      const pageKey = decodeURIComponent(path.slice(`${WS}/view-preferences/`.length))
      if (method === 'PUT') return json({ page_key: pageKey, state: request.postDataJSON().state, state_error: null, updated_at: new Date().toISOString() })
      return json(problem(404, 'not_found', 'No saved preference.', path), 404)
    }
    if (path === `${WS}/view-favorites/order` && method === 'PUT') {
      store.favorites.set(person.id, request.postDataJSON().view_ids)
      return empty()
    }
    if (path === `${WS}/views` && method === 'GET') return json(visibleTo(store, person).map((view) => record(view, person, store)))
    if (path === `${WS}/views` && method === 'POST') {
      const body = request.postDataJSON()
      const view: StoredView = {
        id: `view-${store.views.length + 1}`,
        owner_user_id: person.id,
        name: body.name,
        description: body.description ?? '',
        icon: body.icon ?? null,
        color: body.color ?? null,
        visibility: body.visibility,
        state: body.state,
        version: 1,
      }
      store.views.push(view)
      return json(record(view, person, store), 201)
    }
    const match = path.match(/\/views\/([^/]+)(\/favorite)?$/)
    if (match) {
      const view = visibleTo(store, person).find((candidate) => candidate.id === match[1])
      if (!view) return json(problem(404, 'not_found', 'View not found.', path), 404)
      if (match[2]) {
        const others = (store.favorites.get(person.id) ?? []).filter((id) => id !== view.id)
        store.favorites.set(person.id, method === 'PUT' ? [...others, view.id] : others)
        return empty()
      }
      if (method === 'PATCH') {
        store.patches.push(request.postDataJSON())
        return json(problem(403, 'forbidden', 'Only the owner or an admin can edit this view.', path), 403)
      }
      return json(record(view, person, store))
    }
    return json({ items: [], next_cursor: null })
  })
}

test('build filters, save a workspace view, reload, and favorite it', async ({ page }) => {
  const store = newStore()
  await mockApi(page, OWNER, store)
  await page.goto('/tasks')
  await expect(page.getByRole('button', { name: 'Filter tasks' })).toBeVisible()

  await page.keyboard.press('f')
  await page.getByRole('option', { name: 'Priority' }).click()
  await page.getByRole('option', { name: 'High' }).click()
  await page.keyboard.press('Escape')
  await expect(page.locator('[data-filter-chip="priority"]')).toContainText('High')
  await expect.poll(() => lastQueryFilter(store, OWNER)).toContain('"priority"')

  await page.getByRole('button', { name: 'Save view' }).click()
  const dialog = page.getByRole('dialog', { name: 'Save view' })
  await dialog.getByRole('textbox', { name: 'Name' }).fill('High priority')
  await dialog.getByRole('button', { name: 'Workspace' }).click()
  await dialog.getByRole('button', { name: 'Save view' }).click()

  await expect(page).toHaveURL(/\/views\/view-1$/)
  await expect(page.getByRole('heading', { name: 'High priority' })).toBeVisible()
  expect(store.views[0]).toMatchObject({ name: 'High priority', visibility: 'workspace', owner_user_id: OWNER.id })
  expect(JSON.stringify(store.views[0].state)).toContain('"priority"')

  await page.reload()
  await expect(page.getByRole('heading', { name: 'High priority' })).toBeVisible()
  await expect(page.locator('[data-filter-chip="priority"]')).toContainText('High')
  await expect(page.getByRole('group', { name: 'Unsaved view changes' })).toHaveCount(0)

  await page.getByRole('button', { name: 'Add to favorites' }).click()
  // favorites head the Views page (under Tasks)
  await page.locator('aside').getByRole('link', { name: 'Task views' }).click()
  await expect(page.getByRole('list', { name: 'Favorites' }).getByRole('link', { name: 'High priority' })).toBeVisible()
})

test('a member sees the workspace view and can only save edits as a new view', async ({ browser }) => {
  const store = newStore()
  store.views.push({ id: 'view-1', owner_user_id: OWNER.id, name: 'High priority', description: '', icon: null, color: null, visibility: 'workspace', state: HIGH_STATE, version: 1 })
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } })
  const member = await context.newPage()
  await mockApi(member, MEMBER, store)

  await member.goto('/views')
  await expect(member.getByRole('list', { name: 'Workspace views' })).toBeVisible()
  await member.getByRole('link', { name: 'High priority' }).click()
  await expect(member).toHaveURL(/\/views\/view-1$/)

  await member.getByRole('button', { name: 'Remove priority filter' }).click()
  await expect(member.getByRole('button', { name: 'Save as new view' })).toBeVisible()
  await expect(member.getByRole('button', { name: 'Update view' })).toHaveCount(0)
  await member.getByRole('button', { name: 'View options' }).click()
  await expect(member.getByRole('menuitem', { name: 'Duplicate' })).toBeVisible()
  await expect(member.getByRole('menuitem', { name: 'Edit view' })).toHaveCount(0)
  await member.keyboard.press('Escape')

  await member.getByRole('button', { name: 'Save as new view' }).click()
  const dialog = member.getByRole('dialog', { name: 'Save as new view' })
  await expect(dialog.getByRole('textbox', { name: 'Name' })).toHaveValue('High priority (copy)')
  await dialog.getByRole('button', { name: 'Save as new view' }).click()
  await expect(member).toHaveURL(/\/views\/view-2$/)
  expect(store.patches).toHaveLength(0)
  expect(store.views[1]).toMatchObject({ owner_user_id: MEMBER.id, visibility: 'personal' })
  await context.close()
})

test('preset URLs keep working and send the preset tree', async ({ page }) => {
  const store = newStore()
  await mockApi(page, OWNER, store)
  await page.goto('/tasks?view=overdue')
  await expect(page.getByRole('toolbar', { name: 'Filters' })).toContainText('Overdue')
  await expect.poll(() => lastQueryFilter(store, OWNER)).toContain('"due_date"')
  const filter = lastQueryFilter(store, OWNER)
  expect(filter).toContain('"before"')
  expect(filter).toContain('"status_category"')
})
