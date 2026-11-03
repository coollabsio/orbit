import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { MailServerBanner } from './MailServerBanner'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function renderBanner({ admin, cloud, email }: { admin: boolean; cloud: boolean; email: boolean }) {
  let requests = 0
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    requests += 1
    const path = new URL((input as Request).url).pathname
    if (path === '/api/v1/auth/options') {
      return Response.json({ registration_open: false, email_enabled: email, passkeys_enabled: false, billing_enabled: cloud, providers: [], provider_registration_open: false })
    }
    if (path === '/api/v1/auth/me') {
      return Response.json({ id: 'u1', email: 'root@example.com', display_name: 'Root', installation_admin: admin, root: admin, has_password: true, avatar_url: null })
    }
    return Response.json({}, { status: 404 })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><MemoryRouter><MailServerBanner /></MemoryRouter></QueryClientProvider>)
  return { view, requests: () => requests }
}

const TEXT = 'Orbit Cloud has no mail server. Email change, invitations by email, password reset and sign-up do not work.Set it in Admin → Email'

test('an installation admin on Orbit Cloud without a mail server sees the warning with a link to Admin → Email', async () => {
  const { view } = renderBanner({ admin: true, cloud: true, email: false })
  await waitFor(() => expect(view.getByRole('alert').textContent).toBe(TEXT))
  expect(view.getByRole('link', { name: 'Set it in Admin → Email' }).getAttribute('href')).toBe('/admin/email')
})

for (const [name, flags] of [
  ['a customer', { admin: false, cloud: true, email: false }],
  ['a self-hosted admin', { admin: true, cloud: false, email: false }],
  ['an admin whose Cloud has a mail server', { admin: true, cloud: true, email: true }],
] as const) {
  test(`${name} sees no warning`, async () => {
    const { view, requests } = renderBanner(flags)
    await waitFor(() => expect(requests()).toBe(2))
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(view.queryByRole('alert')).toBeNull()
  })
}
