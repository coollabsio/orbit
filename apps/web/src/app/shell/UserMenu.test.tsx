import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { UserMenu } from './UserMenu'

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}</output>
}

test('account settings navigates to /profile', () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.currentUser, {
    id: 'user-1',
    email: 'owner@orbit.test',
    display_name: 'Owner',
  })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/tasks']}>
        <UserMenu />
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )

  fireEvent.click(view.getByRole('button', { name: 'Account menu for Owner' }))
  expect(view.getByRole('menuitem', { name: 'Appearance' })).toBeTruthy()
  expect(view.getByRole('menuitem', { name: 'Log out' })).toBeTruthy()
  fireEvent.click(view.getByRole('menuitem', { name: 'Account settings' }))
  expect(view.getByTestId('location').textContent).toBe('/profile')
})

test('the sidebar account button shows the name without the email', () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.currentUser, {
    id: 'user-1',
    email: 'owner@orbit.test',
    display_name: 'Owner',
  })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <UserMenu />
      </MemoryRouter>
    </QueryClientProvider>,
  )

  const accountButton = view.getByRole('button', { name: 'Account menu for Owner' })
  expect(accountButton.textContent).toContain('Owner')
  expect(accountButton.textContent).not.toContain('owner@orbit.test')
})
