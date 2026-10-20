import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { AdminSettingsPage } from './AdminSettingsPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const STORAGE = { s3: null, attachments_in_s3: false, backups_in_s3: false, backup_schedule: 'off', files_to_move: 0, move_error: null }
const LIMITS = { max_owned_workspaces: 10, workspace_storage_quota_bytes: 5_000_000_000 }
const SMTP = { host: 'smtp.example.com', port: 587, security: 'starttls', username: 'orbit', password_set: true, from_address: 'orbit@example.com', from_name: 'Orbit' }

test('saving with an empty password field keeps the saved password, and registration needs a mail server', async () => {
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
  const view = render(<QueryClientProvider client={client}><AdminSettingsPage /></QueryClientProvider>)

  const toggle = await view.findByRole('switch', { name: 'Open registration' })
  expect(toggle.getAttribute('aria-disabled') === 'true' || toggle.hasAttribute('data-disabled')).toBe(true)

  await userEvent.type(view.getByLabelText(/^Host/), 'smtp.example.com')
  await userEvent.type(view.getByLabelText(/^From address/), 'orbit@example.com')
  await userEvent.click(view.getByRole('button', { name: 'Save mail settings' }))
  await waitFor(() => expect(bodies).toHaveLength(1))
  expect(bodies[0]).toEqual({ host: 'smtp.example.com', port: 587, security: 'starttls', username: null, from_address: 'orbit@example.com', from_name: null })
  expect(await view.findByRole('button', { name: 'Send test email' })).toBeTruthy()
})

test('limits are saved in bytes, and an empty field is no limit', async () => {
  const bodies: unknown[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PUT') bodies.push({ url: new URL(request.url).pathname, body: await request.json() })
    return Response.json({ registration_open: false, smtp: null, storage: STORAGE, limits: LIMITS })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminSettingsPage /></QueryClientProvider>)

  const workspaces = await view.findByLabelText('Workspaces per account')
  const storage = view.getByLabelText('Storage per workspace (GB)')
  expect((workspaces as HTMLInputElement).value).toBe('10')
  expect((storage as HTMLInputElement).value).toBe('5')
  await userEvent.type(workspaces, '{Backspace}{Backspace}')
  await userEvent.type(storage, '{Backspace}2.5')
  await userEvent.click(view.getByRole('button', { name: 'Save limits' }))
  await waitFor(() => expect(bodies).toHaveLength(1))
  expect(bodies[0]).toEqual({ url: '/api/v1/admin/settings/limits', body: { max_owned_workspaces: null, workspace_storage_quota_bytes: 2_500_000_000 } })
})
