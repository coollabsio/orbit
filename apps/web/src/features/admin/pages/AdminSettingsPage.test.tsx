import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { EnvironmentView } from '@orbit/api/generated/types.gen'
import { AdminSettingsPage } from './AdminSettingsPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const STORAGE = { s3: null, attachments_in_s3: false, backups_in_s3: false, backup_schedule: 'off', files_to_move: 0, move_error: null }
const LIMITS = { max_owned_workspaces: 10, workspace_storage_quota_bytes: 5_000_000_000 }
const ENVIRONMENT: EnvironmentView = { registration_open: false, oidc_provision: false, frame_origins: false, smtp: false, s3: false, attachments_in_s3: false, backups_in_s3: false, backup_schedule: false, s3_keep_deleted_days: false, sign_in_providers: [] }

test('registration needs a mail server', async () => {
  globalThis.fetch = (async () => Response.json({ registration_open: false, smtp: null, frame_origins: [], storage: STORAGE, limits: LIMITS, environment: ENVIRONMENT })) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminSettingsPage /></QueryClientProvider>)

  const toggle = await view.findByRole('switch', { name: 'Open registration' })
  expect(toggle.getAttribute('aria-disabled') === 'true' || toggle.hasAttribute('data-disabled')).toBe(true)
})

test('registration set by the environment cannot be changed here', async () => {
  const smtp = { host: 'smtp.example.com', port: 587, security: 'starttls', username: null, password_set: false, from_address: 'orbit@example.com', from_name: null }
  globalThis.fetch = (async () => Response.json({ registration_open: true, smtp, frame_origins: [], storage: STORAGE, limits: LIMITS, environment: { ...ENVIRONMENT, registration_open: true } })) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminSettingsPage /></QueryClientProvider>)

  const toggle = await view.findByRole('switch', { name: 'Open registration' })
  expect(toggle.getAttribute('aria-checked')).toBe('true')
  expect(toggle.getAttribute('aria-disabled') === 'true' || toggle.hasAttribute('data-disabled')).toBe(true)
  expect(view.getByText('ORBIT_OPEN_REGISTRATION')).toBeTruthy()
})

test('the root user saves the custom page origins, one per line', async () => {
  const bodies: unknown[] = []
  let origins = ['https://grafana.example.com']
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    if (request.method === 'PUT') {
      const body = await request.json() as { origins: string[] }
      bodies.push(body)
      origins = body.origins
    }
    return Response.json({ registration_open: false, smtp: null, frame_origins: origins, storage: STORAGE, limits: LIMITS, environment: ENVIRONMENT })
  }) as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminSettingsPage /></QueryClientProvider>)

  const field = await view.findByRole('textbox', { name: 'Custom page origins' }) as HTMLTextAreaElement
  expect(field.value).toBe('https://grafana.example.com')
  const save = view.getByRole('button', { name: 'Save' }) as HTMLButtonElement
  expect(save.disabled).toBe(true)
  fireEvent.change(field, { target: { value: 'https://grafana.example.com\n\n  https://deploys.example.com ' } })
  fireEvent.click(save)
  await waitFor(() => expect(bodies).toEqual([{ origins: ['https://grafana.example.com', 'https://deploys.example.com'] }]))
})

test('custom page origins set by the environment cannot be changed here', async () => {
  globalThis.fetch = (async () => Response.json({ registration_open: false, smtp: null, frame_origins: ['https://env.example.com'], storage: STORAGE, limits: LIMITS, environment: { ...ENVIRONMENT, frame_origins: true } })) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminSettingsPage /></QueryClientProvider>)

  const field = await view.findByRole('textbox', { name: 'Custom page origins' }) as HTMLTextAreaElement
  expect(field.disabled).toBe(true)
  expect(view.getByText('ORBIT_FRAME_ORIGINS')).toBeTruthy()
  expect(view.queryByRole('button', { name: 'Save' })).toBeNull()
})
