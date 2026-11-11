import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { EnvironmentView } from '@orbit/api/generated/types.gen'
import { AdminSettingsPage } from './AdminSettingsPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const STORAGE = { s3: null, attachments_in_s3: false, backups_in_s3: false, backup_schedule: 'off', files_to_move: 0, move_error: null }
const LIMITS = { max_owned_workspaces: 10, workspace_storage_quota_bytes: 5_000_000_000 }
const ENVIRONMENT: EnvironmentView = { registration_open: false, smtp: false, s3: false, attachments_in_s3: false, backups_in_s3: false, backup_schedule: false, sign_in_providers: [] }

test('registration needs a mail server', async () => {
  globalThis.fetch = (async () => Response.json({ registration_open: false, smtp: null, storage: STORAGE, limits: LIMITS, environment: ENVIRONMENT })) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminSettingsPage /></QueryClientProvider>)

  const toggle = await view.findByRole('switch', { name: 'Open registration' })
  expect(toggle.getAttribute('aria-disabled') === 'true' || toggle.hasAttribute('data-disabled')).toBe(true)
})

test('registration set by the environment cannot be changed here', async () => {
  const smtp = { host: 'smtp.example.com', port: 587, security: 'starttls', username: null, password_set: false, from_address: 'orbit@example.com', from_name: null }
  globalThis.fetch = (async () => Response.json({ registration_open: true, smtp, storage: STORAGE, limits: LIMITS, environment: { ...ENVIRONMENT, registration_open: true } })) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminSettingsPage /></QueryClientProvider>)

  const toggle = await view.findByRole('switch', { name: 'Open registration' })
  expect(toggle.getAttribute('aria-checked')).toBe('true')
  expect(toggle.getAttribute('aria-disabled') === 'true' || toggle.hasAttribute('data-disabled')).toBe(true)
  expect(view.getByText('ORBIT_OPEN_REGISTRATION')).toBeTruthy()
})
