import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { AdminEmailPage } from './AdminEmailPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const STORAGE = { s3: null, attachments_in_s3: false, backups_in_s3: false, backup_schedule: 'off', files_to_move: 0, move_error: null }
const LIMITS = { max_owned_workspaces: 10, workspace_storage_quota_bytes: 5_000_000_000 }
const SMTP = { host: 'smtp.example.com', port: 587, security: 'starttls', username: 'orbit', password_set: true, from_address: 'orbit@example.com', from_name: 'Orbit' }

test('saving with an empty password field keeps the saved password', async () => {
  const bodies: unknown[] = []
  let settings: object = { registration_open: false, smtp: null, storage: STORAGE, limits: LIMITS }
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PUT') {
      const body = await request.json()
      bodies.push(body)
      settings = { registration_open: false, smtp: SMTP, storage: STORAGE, limits: LIMITS }
    }
    return Response.json(settings)
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminEmailPage /></QueryClientProvider>)

  await userEvent.type(await view.findByLabelText(/^Host/), 'smtp.example.com')
  await userEvent.type(view.getByLabelText(/^From address/), 'orbit@example.com')
  await userEvent.click(view.getByRole('button', { name: 'Save mail settings' }))
  await waitFor(() => expect(bodies).toHaveLength(1))
  expect(bodies[0]).toEqual({ host: 'smtp.example.com', port: 587, security: 'starttls', username: null, from_address: 'orbit@example.com', from_name: null })
  expect(await view.findByRole('button', { name: 'Send test email' })).toBeTruthy()
})

function renderSaved(billingEnabled: boolean) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const path = new URL((input as Request).url).pathname
    if (path === '/api/v1/auth/options') {
      return Response.json({ registration_open: false, email_enabled: true, passkeys_enabled: false, billing_enabled: billingEnabled, providers: [], provider_registration_open: false })
    }
    return Response.json({ registration_open: false, smtp: SMTP, storage: STORAGE, limits: LIMITS })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><AdminEmailPage /></QueryClientProvider>)
}

test('Orbit Cloud offers no Remove and says why', async () => {
  const view = renderSaved(true)
  expect(await view.findByText('Orbit Cloud needs a mail server. Save a different one instead.')).toBeTruthy()
  expect(view.queryByRole('button', { name: 'Remove' })).toBeNull()
})

test('a self-hosted server can remove its mail server', async () => {
  const view = renderSaved(false)
  expect(await view.findByRole('button', { name: 'Remove' })).toBeTruthy()
  expect(view.queryByText('Orbit Cloud needs a mail server. Save a different one instead.')).toBeNull()
})
