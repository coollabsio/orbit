import { expect, test, type Page } from '@playwright/test'
import { keepIdentifiersTogether } from '../src/lib/toast'
import { testWorkspace } from '../src/test/workspace'

test.use({ viewport: { width: 1280, height: 800 } })

type TaskMock = { id: string; title: string; status_id: string; parent_task_id: string | null; version: number; position: number; created_at: string; deleted_at: string | null }
type Write = { method: string; path: string; body: unknown }

const CLOSED = new Set(['done'])
const base = {
  workspace_id: 'alpha', project_id: 'project-1', description: '', priority: 'none', assignee_ids: [], label_ids: [],
  creator_id: 'user-1', blocked: false, duplicate_of: null, updated_at: '2026-09-27T12:00:00Z',
}
const mock = (id: string, title: string, parent: string | null = null, position = 1): TaskMock =>
  ({ id, title, status_id: 'todo', parent_task_id: parent, version: 1, position, created_at: `2026-09-27T10:00:0${position}Z`, deleted_at: null })

const live = (tasks: TaskMock[]) => tasks.filter((task) => !task.deleted_at)
const childrenOf = (tasks: TaskMock[], id: string) => live(tasks).filter((task) => task.parent_task_id === id)
function descendants(tasks: TaskMock[], id: string): TaskMock[] {
  return tasks.filter((task) => task.parent_task_id === id).flatMap((task) => [task, ...descendants(tasks, task.id)])
}
function view(tasks: TaskMock[], task: TaskMock, withAncestors = false) {
  const parent = live(tasks).find((item) => item.id === task.parent_task_id)
  const kids = childrenOf(tasks, task.id)
  const ancestors: Array<{ id: string; title: string; project_key: string }> = []
  for (let up = parent; up; up = live(tasks).find((item) => item.id === up!.parent_task_id)) ancestors.unshift({ id: up.id, title: up.title, project_key: 'TEST' })
  return {
    ...base, ...task,
    parent: parent ? { id: parent.id, title: parent.title, project_key: 'TEST' } : null,
    sub_issue_count: kids.length,
    sub_issue_closed_count: kids.filter((kid) => CLOSED.has(kid.status_id)).length,
    ...(withAncestors ? { ancestors } : {}),
  }
}
/** Rule A in miniature: when every live child is closed, the parent moves to Done, repeating upward. */
function autoCloseParents(tasks: TaskMock[], task: TaskMock) {
  const closed: Array<{ id: string; status_id: string }> = []
  let parent = live(tasks).find((item) => item.id === task.parent_task_id)
  while (parent && !CLOSED.has(parent.status_id) && childrenOf(tasks, parent.id).every((kid) => CLOSED.has(kid.status_id))) {
    Object.assign(parent, { status_id: 'done', version: parent.version + 1 })
    closed.push({ id: parent.id, status_id: 'done' })
    parent = live(tasks).find((item) => item.id === parent!.parent_task_id)
  }
  return closed
}

async function mockApi(page: Page, tasks: TaskMock[], writes: Write[]) {
  let created = 100
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request()
    const method = request.method()
    const url = new URL(request.url())
    const path = url.pathname
    const find = (id: string) => tasks.find((task) => task.id === id)!
    // task queries are reads sent as POST: keep them out of the recorded writes
    if (method !== 'GET' && !path.endsWith('/tasks/query') && request.postData()) writes.push({ method, path, body: request.postDataJSON() })
    if (method === 'PATCH' && /\/tasks\/[^/]+$/.test(path)) {
      const task = find(path.split('/').at(-1)!)
      const body = request.postDataJSON() as { status_id?: string; parent_task_id?: string | null }
      if (body.status_id) task.status_id = body.status_id
      if ('parent_task_id' in body) task.parent_task_id = body.parent_task_id ?? null
      task.version += 1
      await route.fulfill({ json: { ...view(tasks, task), auto_closed: body.status_id ? autoCloseParents(tasks, task) : [] } })
      return
    }
    if (method === 'POST' && path.endsWith('/tasks')) {
      const body = request.postDataJSON() as { title: string; parent_task_id?: string }
      created += 1
      const task = { ...mock(`task-0${created}`, body.title, body.parent_task_id ?? null, created), created_at: `2026-09-27T11:00:${created - 100}0Z` }
      tasks.push(task)
      await route.fulfill({ status: 201, json: view(tasks, task) })
      return
    }
    if (method === 'DELETE' && /\/tasks\/[^/]+$/.test(path)) {
      const task = find(path.split('/').at(-1)!)
      const stamp = new Date().toISOString()
      for (const item of [task, ...descendants(tasks, task.id).filter((child) => !child.deleted_at)]) item.deleted_at = stamp
      await route.fulfill({ status: 204 })
      return
    }
    if (method === 'POST' && path.endsWith('/restore')) {
      const task = find(path.split('/').at(-2)!)
      const stamp = task.deleted_at
      for (const item of [task, ...descendants(tasks, task.id).filter((child) => child.deleted_at === stamp)]) item.deleted_at = null
      task.version += 1
      await route.fulfill({ json: view(tasks, task) })
      return
    }
    if (/\/views\/[^/]+\/favorite$|\/view-favorites\/order$/.test(path)) { await route.fulfill({ status: 204 }); return }
    let body: unknown = { items: [], next_cursor: null }
    if (path.endsWith('/views')) body = []
    if (path.endsWith('/setup/status')) body = { complete: true }
    if (path.endsWith('/auth/me')) body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
    if (path === '/api/v1/workspaces') body = [testWorkspace('owner', { id: 'alpha', name: 'Alpha' })]
    if (path.endsWith('/projects')) body = { items: [{ id: 'project-1', name: 'Launch', key: 'TEST', color: '#e0457b', version: 1, auto_close_parent: true, auto_close_sub_issues: true }], next_cursor: null }
    if (path.endsWith('/statuses')) {
      body = { items: [
        { id: 'todo', project_id: 'project-1', name: 'Todo', description: '', category: 'unstarted', color: '#888888', position: 0, version: 1 },
        { id: 'done', project_id: 'project-1', name: 'Done', description: '', category: 'completed', color: '#4cb782', position: 1, version: 1 },
      ], next_cursor: null }
    }
    if (path.endsWith('/tasks/trash')) {
      // ruling F7: the real server lists every trashed row (parent AND its trashed sub-issues), each with its own
      // trashed_descendant_count — see apps/server/tests/tasks_api.rs ~5250-5269 (parent, child, earlier-trashed task
      // are three separate items). The web only shows a "+N sub-issues" label on rows that have one.
      const trashed = tasks.filter((task) => task.deleted_at)
      body = { items: trashed.map((task) => ({ ...view(tasks, task), trashed_descendant_count: descendants(tasks, task.id).filter((item) => item.deleted_at === task.deleted_at).length })), next_cursor: null }
    } else if (path.endsWith('/tasks') || path.endsWith('/tasks/query')) {
      const parentId = url.searchParams.get('parent_task_id')
      const items = parentId ? childrenOf(tasks, parentId).sort((a, b) => a.created_at.localeCompare(b.created_at)) : live(tasks)
      body = { items: items.map((task) => view(tasks, task)), next_cursor: null }
    } else {
      const detail = path.match(/\/tasks\/([^/]+)$/)
      if (detail) {
        const task = live(tasks).find((item) => item.id === detail[1])
        if (!task) { await route.fulfill({ status: 404, json: { type: 'about:blank', title: 'Not found', status: 404, detail: 'missing', code: 'task_resource_not_found', instance: path, request_id: 'r' } }); return }
        body = view(tasks, task, true)
      }
    }
    if (path.endsWith('/relations') || path.endsWith('/github-links')) body = []
    await route.fulfill({ json: body })
  })
}

test('add two sub-issues inline, close both, and the parent closes itself', async ({ page }) => {
  const tasks = [mock('task-0012', 'Checkout redesign')]
  const writes: Write[] = []
  await mockApi(page, tasks, writes)
  await page.goto('/tasks/task-0012?workspace=alpha')

  await page.getByRole('button', { name: 'Add sub-issue' }).click()
  const composer = page.getByRole('textbox', { name: 'Sub-issue title' })
  const section = page.getByRole('region', { name: 'Sub-issues' })
  await composer.fill('Payment form')
  await composer.press('Enter')
  await expect(section.getByText('Payment form')).toBeVisible()
  await expect(composer).toBeFocused()
  await expect(composer).toHaveValue('')
  await composer.fill('Receipt email')
  await composer.press('Enter')
  await expect(section.getByText('Receipt email')).toBeVisible()
  await composer.press('Escape')
  await expect(composer).toHaveCount(0)
  await expect(page).toHaveURL(/\/tasks\/task-0012/)
  expect(writes.filter((write) => write.method === 'POST')).toHaveLength(2)
  await expect(section.getByRole('img', { name: '0 of 2 sub-issues closed' })).toBeVisible()

  for (const title of ['Payment form', 'Receipt email']) {
    await section.getByRole('listitem').filter({ hasText: title }).getByRole('button', { name: 'Status: Todo' }).click()
    await page.getByRole('menuitem', { name: /Done$/ }).click()
  }
  await expect(page.getByText(keepIdentifiersTogether('Closed parent TEST-0012'))).toBeVisible()
  await expect(section.getByRole('img', { name: '2 of 2 sub-issues closed' })).toBeVisible()
})

test('drag a task onto another in the list to nest it, then undo', async ({ page }) => {
  const tasks = [mock('task-0012', 'Checkout redesign', null, 1), mock('task-0033', 'Receipt email', null, 2)]
  const writes: Write[] = []
  await mockApi(page, tasks, writes)
  await page.goto('/tasks?workspace=alpha')

  await page.locator('[data-task-row]', { hasText: 'Receipt email' }).dragTo(page.locator('[data-task-row]', { hasText: 'Checkout redesign' }))
  await expect(page.getByText(keepIdentifiersTogether('TEST-0033 is now a sub-issue of TEST-0012'))).toBeVisible()
  await expect(page.locator('[data-task-row][data-depth="1"]', { hasText: 'Receipt email' })).toBeVisible()
  expect(writes).toContainEqual({ method: 'PATCH', path: '/api/v1/workspaces/alpha/tasks/task-0033', body: { expected_version: 1, parent_task_id: 'task-0012' } })

  await page.getByRole('button', { name: 'Undo' }).click()
  await expect(page.locator('[data-task-row][data-depth="0"]', { hasText: 'Receipt email' })).toBeVisible()
  expect(writes.at(-1)).toEqual({ method: 'PATCH', path: '/api/v1/workspaces/alpha/tasks/task-0033', body: { expected_version: 2, parent_task_id: null } })
})

test('deleting a parent moves its subtree to trash; restoring brings the subtree back', async ({ page }) => {
  const tasks = [mock('task-0012', 'Checkout redesign', null, 1), mock('task-0031', 'Payment form', 'task-0012', 2), mock('task-0032', 'Receipt email', 'task-0012', 3)]
  await mockApi(page, tasks, [])
  await page.goto('/tasks/task-0012?workspace=alpha')

  await page.getByRole('button', { name: 'Delete', exact: true }).click()
  await page.getByRole('button', { name: 'Move to trash', exact: true }).click()
  await page.goto('/tasks-trash?workspace=alpha')
  // ruling F7: the mocked trash endpoint lists every trashed row (parent AND its trashed sub-issues), exactly like
  // the real server (apps/server/tests/tasks_api.rs ~5250-5269) — only the parent's row carries the "+N sub-issues"
  // annotation, and restoring it must bring the whole subtree back with it.
  await expect(page.getByText('Payment form')).toBeVisible()
  await expect(page.getByText('Receipt email')).toBeVisible()
  const parentRow = page.locator('div').filter({ hasText: 'Checkout redesign' }).filter({ hasText: '+2 sub-issues' }).last()
  await expect(parentRow.getByText('+2 sub-issues')).toBeVisible()

  await parentRow.getByRole('button', { name: 'Restore', exact: true }).click()
  await expect(page.getByText('Trash is empty')).toBeVisible()
  await page.goto('/tasks?workspace=alpha')
  await expect(page.locator('[data-task-row][data-depth="1"]')).toHaveCount(2)
})
