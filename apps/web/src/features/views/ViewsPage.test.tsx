import { afterEach, expect, test } from 'bun:test'
import { fireEvent, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders, savedView, stubFetch } from './testUtils'
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

test('splits personal and workspace views into tabs of flat rows', async () => {
  const { view } = renderWithProviders(<ViewsPage />, { views: [MINE, SHARED] })
  const panel = () => view.getByRole('tabpanel')
  expect(within(panel()).getByRole('link', { name: 'My bugs' }).getAttribute('href')).toBe('/views/view-mine')
  expect(within(panel()).getByText('Bugs assigned to me')).toBeTruthy()
  expect(within(panel()).queryByRole('link', { name: 'Launch board' })).toBeNull()
  await userEvent.click(view.getByRole('tab', { name: 'Workspace views' }))
  expect(within(panel()).getByRole('link', { name: 'Launch board' })).toBeTruthy()
  expect(within(panel()).getByRole('button', { name: 'Remove Launch board from favorites' }).getAttribute('aria-pressed')).toBe('true')
})

test('the row menu copies a link and hides edit actions from non-editors', async () => {
  let copied = ''
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text: string) => { copied = text } } })
  const { view } = renderWithProviders(<ViewsPage />, { views: [SHARED] })
  await userEvent.click(view.getByRole('tab', { name: 'Workspace views' }))
  fireEvent.click(view.getByRole('button', { name: 'Launch board options' }))
  expect(await view.findByRole('menuitem', { name: 'Duplicate' })).toBeTruthy()
  expect(view.queryByRole('menuitem', { name: 'Edit view' })).toBeNull()
  expect(view.queryByRole('menuitem', { name: 'Delete view' })).toBeNull()
  await userEvent.click(view.getByRole('menuitem', { name: 'Copy link' }))
  await waitFor(() => expect(copied).toBe('http://localhost/views/view-shared'))
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
  expect(view.getByText('Save a filter from any task page to create a view')).toBeTruthy()
  await userEvent.click(within(view.getByRole('tabpanel')).getByRole('button', { name: 'New view' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks?save_view=1')
})
