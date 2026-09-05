import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { ReactNode } from 'react'
import { queryKeys } from '../../api/queryKeys'
import { sessionView } from './api/sessions'
import { SessionsPage } from './SessionsPage'

test('mounted sessions never offer to revoke the server-identified current session', () => {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } })
  const record = (id: string, current: boolean) => sessionView({
    id, current, created_at: '2026-09-05T10:00:00Z', last_activity_at: '2026-09-05T10:00:00Z',
    idle_expires_at: '2026-09-06T10:00:00Z', absolute_expires_at: '2026-10-05T10:00:00Z',
    user: { id: 'user-1', email: 'user@orbit.test', display_name: 'User' },
  })
  client.setQueryData(queryKeys.sessions, [record('current', true), record('other', false)])
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  const view = render(<SessionsPage />, { wrapper })

  expect(view.getAllByRole('button', { name: 'Revoke' })).toHaveLength(1)
  expect(view.getByText('Current')).toBeTruthy()
})
