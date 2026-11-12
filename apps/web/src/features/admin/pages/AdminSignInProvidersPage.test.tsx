import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { EnvironmentView } from '@orbit/api/generated/types.gen'
import { AdminSignInProvidersPage } from './AdminSignInProvidersPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const ENVIRONMENT: EnvironmentView = { registration_open: false, smtp: false, s3: false, attachments_in_s3: false, backups_in_s3: false, backup_schedule: false, s3_keep_deleted_days: false, sign_in_providers: [] }
const GOOGLE = { provider: 'google', client_id: 'google-client', secret_set: true, callback_url: 'https://orbit.example.com/api/v1/auth/providers/google/callback' }

test('a provider set by the environment is read-only, the other stays editable', async () => {
  globalThis.fetch = (async () => Response.json({ registration_open: false, smtp: null, sign_in_providers: [GOOGLE], environment: { ...ENVIRONMENT, sign_in_providers: ['google'] } })) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminSignInProvidersPage /></QueryClientProvider>)

  const google = (await view.findByRole('button', { name: 'Save Google' })) as HTMLButtonElement
  expect(google.disabled).toBe(true)
  expect((view.getByRole('button', { name: 'Remove' }) as HTMLButtonElement).disabled).toBe(true)
  expect((view.getByRole('button', { name: 'Save GitHub' }) as HTMLButtonElement).disabled).toBe(false)
  const clientIds = view.getAllByLabelText(/^Client ID/) as HTMLInputElement[]
  expect(clientIds.map((input) => input.disabled)).toEqual([true, false])
  expect(view.getByText('ORBIT_GOOGLE_*')).toBeTruthy()
})
