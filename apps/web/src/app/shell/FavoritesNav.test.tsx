import { afterEach, expect, test } from 'bun:test'
import { fireEvent, waitFor } from '@testing-library/react'
import { renderWithProviders, savedView, stubFetch } from '@/features/views/testUtils'
import { FavoritesNav } from './FavoritesNav'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const ALPHA = savedView({ id: 'view-a', name: 'Alpha view', is_favorite: true, favorite_position: 1 })
const BETA = savedView({ id: 'view-b', name: 'Beta view', is_favorite: true, favorite_position: 0 })
const PLAIN = savedView({ id: 'view-c', name: 'Not a favorite' })
const names = (links: HTMLElement[]) => links.map((link) => link.textContent)

test('renders nothing without favorites', () => {
  const { view } = renderWithProviders(<FavoritesNav />, { views: [PLAIN] })
  expect(view.queryByText('Favorites')).toBeNull()
})

test('lists favorites in saved order and links to each view', () => {
  const { view } = renderWithProviders(<FavoritesNav />, { views: [ALPHA, BETA, PLAIN] })
  expect(view.getByText('Favorites')).toBeTruthy()
  expect(names(view.getAllByRole('link'))).toEqual(['Beta view', 'Alpha view'])
  expect(view.getByRole('link', { name: 'Alpha view' }).getAttribute('href')).toBe('/views/view-a')
})

test('keeps an accessible name for each favorite when the sidebar is collapsed', () => {
  const { view } = renderWithProviders(<FavoritesNav collapsed />, { views: [ALPHA] })
  expect(view.getByRole('link', { name: 'Alpha view' }).getAttribute('title')).toBe('Alpha view')
})

test('tolerates a views response that is not a list', () => {
  const { view } = renderWithProviders(<FavoritesNav />, { views: { items: [], next_cursor: null } as never })
  expect(view.queryByText('Favorites')).toBeNull()
})

test('dragging a favorite keeps it mounted and saves the full order', async () => {
  const requests = stubFetch((request) => (request.method === 'GET' ? Response.json([ALPHA, BETA, PLAIN]) : new Response(null, { status: 204 })))
  const { view } = renderWithProviders(<FavoritesNav />, { views: [ALPHA, BETA, PLAIN] })
  const alpha = view.getByRole('link', { name: 'Alpha view' })
  const beta = view.getByRole('link', { name: 'Beta view' })
  const dataTransfer = { setData: () => {}, getData: () => '', effectAllowed: 'all', dropEffect: 'none' }
  fireEvent.dragStart(alpha, { dataTransfer })
  fireEvent.dragOver(beta, { dataTransfer })
  expect(alpha.getAttribute('data-dragging')).toBe('true')
  expect(beta.getAttribute('data-drop-target')).toBe('true')
  fireEvent.drop(beta, { dataTransfer })
  expect(names(view.getAllByRole('link'))).toEqual(['Alpha view', 'Beta view'])
  await waitFor(() => expect(requests.find((request) => request.method === 'PUT')?.body).toEqual({ view_ids: ['view-a', 'view-b'] }))
})

test('Alt+Arrow keys reorder from the keyboard', async () => {
  const requests = stubFetch((request) => (request.method === 'GET' ? Response.json([ALPHA, BETA]) : new Response(null, { status: 204 })))
  const { view } = renderWithProviders(<FavoritesNav />, { views: [ALPHA, BETA] })
  fireEvent.keyDown(view.getByRole('link', { name: 'Beta view' }), { key: 'ArrowDown', altKey: true })
  await waitFor(() => expect(requests.find((request) => request.method === 'PUT')?.body).toEqual({ view_ids: ['view-a', 'view-b'] }))
})
