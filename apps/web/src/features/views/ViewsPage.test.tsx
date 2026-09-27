import { afterEach, expect, test } from 'bun:test'
import { fireEvent, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders, savedView, stubFetch } from './testUtils'
import { DEFAULT_DISPLAY } from './viewState'
import { ViewsPage } from './ViewsPage'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const MINE = savedView({ id: 'view-mine', name: 'My bugs', description: 'Bugs assigned to me' })
const SHARED = savedView({
  id: 'view-shared',
  name: 'Launch board',
  visibility: 'workspace',
  owner: { user_id: 'user-2', display_name: 'Grace' },
  can_edit: false,
  is_favorite: true,
  favorite_position: 0,
})

test('the header places views under Tasks and links back', () => {
  const { view } = renderWithProviders(<ViewsPage />, { views: [] })
  const crumbs = view.getByRole('navigation', { name: 'Breadcrumb' })
  expect(within(crumbs).getByRole('link', { name: 'Tasks' }).getAttribute('href')).toBe('/tasks')
  expect(within(crumbs).getByRole('heading', { name: 'Views' })).toBeTruthy()
})

const SHARED_PLAIN = savedView({
  id: 'view-roadmap',
  name: 'Roadmap',
  visibility: 'workspace',
  state: {
    filter: { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['high'] }, { op: 'or', children: [{ field: 'assignee', operator: 'is', value: ['me'] }, { field: 'label', operator: 'includes_any', value: ['l1'] }] }] },
    display: { ...DEFAULT_DISPLAY, layout: 'board', group_by: 'assignee' },
  },
})
const section = (view: ReturnType<typeof renderWithProviders>['view'], name: string) => view.getByRole('list', { name })

test('one list in sections: favorites first, then your views, then workspace views, each view once', () => {
  const { view } = renderWithProviders(<ViewsPage />, { views: [MINE, SHARED, SHARED_PLAIN] })
  expect(view.getAllByRole('heading', { level: 2 }).map((heading) => heading.textContent)).toEqual(['Favorites', 'Your views', 'Workspace views'])
  expect(within(section(view, 'Favorites')).getByRole('link', { name: 'Launch board' })).toBeTruthy()
  expect(within(section(view, 'Your views')).getByRole('link', { name: 'My bugs' }).getAttribute('href')).toBe('/views/view-mine')
  expect(within(section(view, 'Your views')).getByText('Bugs assigned to me')).toBeTruthy()
  // the starred shared view is not listed again under Workspace views
  expect(within(section(view, 'Workspace views')).getAllByRole('listitem')).toHaveLength(1)
  expect(view.getByRole('button', { name: 'Remove Launch board from favorites' }).getAttribute('aria-pressed')).toBe('true')
})

test('a row says what the view shows: its filter fields and its layout', () => {
  const { view } = renderWithProviders(<ViewsPage />, { views: [MINE, SHARED_PLAIN] })
  const roadmap = within(section(view, 'Workspace views')).getByRole('listitem')
  expect(within(roadmap).getByText('Priority, Assignee +1')).toBeTruthy()
  expect(within(roadmap).getByText('Board · Assignee')).toBeTruthy()
  const bugs = within(section(view, 'Your views')).getByRole('listitem')
  expect(within(bugs).getByText('All tasks')).toBeTruthy()
  expect(within(bugs).getByText('List · Status')).toBeTruthy()
})

test('search narrows every section by name and description', async () => {
  const { view } = renderWithProviders(<ViewsPage />, { views: [MINE, SHARED, SHARED_PLAIN] })
  await userEvent.type(view.getByRole('searchbox', { name: 'Search views' }), 'assigned')
  expect(view.getAllByRole('heading', { level: 2 }).map((heading) => heading.textContent)).toEqual(['Your views'])
  await userEvent.clear(view.getByRole('searchbox', { name: 'Search views' }))
  await userEvent.type(view.getByRole('searchbox', { name: 'Search views' }), 'zzz')
  expect(view.getByText('No views match “zzz”.')).toBeTruthy()
})

test('the row menu copies a link and hides edit actions from non-editors', async () => {
  let copied = ''
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text: string) => { copied = text } } })
  const { view } = renderWithProviders(<ViewsPage />, { views: [SHARED] })
  fireEvent.click(view.getByRole('button', { name: 'Launch board options' }))
  expect(await view.findByRole('menuitem', { name: 'Duplicate' })).toBeTruthy()
  expect(view.queryByRole('menuitem', { name: 'Edit view' })).toBeNull()
  expect(view.queryByRole('menuitem', { name: 'Delete view' })).toBeNull()
  await userEvent.click(view.getByRole('menuitem', { name: 'Copy link' }))
  // the workspace goes along, so the link opens in the right workspace
  await waitFor(() => expect(copied).toBe('http://localhost/views/view-shared?workspace=alpha'))
})

test('deleting a view asks for confirmation first', async () => {
  const requests = stubFetch((request) => (request.method === 'DELETE' ? new Response(null, { status: 204 }) : Response.json([])))
  const { view } = renderWithProviders(<ViewsPage />, { views: [MINE] })
  fireEvent.click(view.getByRole('button', { name: 'My bugs options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Delete view' }))
  expect(await view.findByText('Delete view?')).toBeTruthy()
  expect(requests.some((request) => request.method === 'DELETE')).toBe(false)
  await userEvent.click(view.getByRole('button', { name: 'Delete view' }))
  await waitFor(() => expect(requests.some((request) => request.method === 'DELETE' && request.path.endsWith('/views/view-mine'))).toBe(true))
})

test('the empty state points to saving a filter and starts a new view on /tasks', async () => {
  const { view } = renderWithProviders(<ViewsPage />, { views: [] })
  expect(view.getByText('Filter or rearrange any task page, then save it as a view to come back to it.')).toBeTruthy()
  expect(view.queryByRole('searchbox')).toBeNull()
  await userEvent.click(view.getAllByRole('button', { name: 'New view' })[1] as HTMLElement)
  expect(view.getByTestId('location').textContent).toBe('/tasks?save_view=1')
})

const ALPHA = savedView({ id: 'view-a', name: 'Alpha view', is_favorite: true, favorite_position: 1 })
const BETA = savedView({ id: 'view-b', name: 'Beta view', is_favorite: true, favorite_position: 0 })
const PLAIN = savedView({ id: 'view-p', name: 'Plain view' })
const favoriteRows = (view: ReturnType<typeof renderWithProviders>['view']) =>
  within(view.getByRole('list', { name: 'Favorites' })).getAllByRole('listitem')
const names = (rows: HTMLElement[]) => rows.map((row) => within(row).getAllByRole('link')[0]?.textContent)

test('Favorites lists only starred views, in favorite order', () => {
  const { view } = renderWithProviders(<ViewsPage />, { views: [ALPHA, PLAIN, BETA] })
  expect(names(favoriteRows(view))).toEqual(['Beta view', 'Alpha view'])
})

test('dragging a favorite keeps it mounted and saves the full order', async () => {
  const requests = stubFetch((request) => (request.method === 'GET' ? Response.json([ALPHA, BETA, PLAIN]) : new Response(null, { status: 204 })))
  const { view } = renderWithProviders(<ViewsPage />, { views: [ALPHA, BETA, PLAIN] })
  const [beta, alpha] = favoriteRows(view) as [HTMLElement, HTMLElement]
  const dataTransfer = { setData: () => {}, getData: () => '', effectAllowed: 'all', dropEffect: 'none' }
  fireEvent.dragStart(alpha, { dataTransfer })
  fireEvent.dragOver(beta, { dataTransfer })
  expect(alpha.getAttribute('data-dragging')).toBe('true')
  expect(beta.getAttribute('data-drop-target')).toBe('true')
  expect(beta.getAttribute('data-drop-edge')).toBe('before')
  fireEvent.drop(beta, { dataTransfer })
  expect(names(favoriteRows(view))).toEqual(['Alpha view', 'Beta view'])
  await waitFor(() => expect(requests.find((request) => request.method === 'PUT')?.body).toEqual({ view_ids: ['view-a', 'view-b'] }))
})

test('dragging a favorite downward marks the drop below the target', () => {
  const { view } = renderWithProviders(<ViewsPage />, { views: [ALPHA, BETA] })
  const [beta, alpha] = favoriteRows(view) as [HTMLElement, HTMLElement]
  const dataTransfer = { setData: () => {}, getData: () => '', effectAllowed: 'all', dropEffect: 'none' }
  fireEvent.dragStart(beta, { dataTransfer })
  fireEvent.dragOver(alpha, { dataTransfer })
  expect(alpha.getAttribute('data-drop-target')).toBe('true')
  expect(alpha.getAttribute('data-drop-edge')).toBe('after')
  fireEvent.dragEnd(beta, { dataTransfer })
  expect(alpha.getAttribute('data-drop-edge')).toBeNull()
})

test('Alt+Arrow keys reorder favorites from the keyboard', async () => {
  const requests = stubFetch((request) => (request.method === 'GET' ? Response.json([ALPHA, BETA]) : new Response(null, { status: 204 })))
  const { view } = renderWithProviders(<ViewsPage />, { views: [ALPHA, BETA] })
  fireEvent.keyDown(view.getByRole('link', { name: 'Beta view' }), { key: 'ArrowDown', altKey: true })
  await waitFor(() => expect(requests.find((request) => request.method === 'PUT')?.body).toEqual({ view_ids: ['view-a', 'view-b'] }))
})

test('a search turns off reordering, which would send only part of the order', async () => {
  const { view } = renderWithProviders(<ViewsPage />, { views: [ALPHA, BETA] })
  expect(favoriteRows(view)[0]?.getAttribute('draggable')).toBe('true')
  await userEvent.type(view.getByRole('searchbox', { name: 'Search views' }), 'view')
  expect(favoriteRows(view)[0]?.getAttribute('draggable')).toBe('false')
})
