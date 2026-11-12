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

test('OpenID Connect shows the environment values without a save, or the variables that turn it on', async () => {
  const OIDC = { provider: 'oidc', client_id: 'orbit', secret_set: true, issuer: 'https://auth.example.com', callback_url: 'https://orbit.example.com/api/v1/auth/providers/oidc/callback' }
  globalThis.fetch = (async (input: Request) => new URL(input.url).pathname === '/api/v1/auth/options'
    ? Response.json({ providers: ['oidc'], oidc_name: 'Authentik' })
    : Response.json({ registration_open: false, smtp: null, sign_in_providers: [OIDC], environment: { ...ENVIRONMENT, sign_in_providers: ['oidc'] } })) as unknown as typeof fetch
  const set = render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><AdminSignInProvidersPage /></QueryClientProvider>)

  const issuer = (await set.findByLabelText('Issuer')) as HTMLInputElement
  expect([issuer.value, issuer.disabled]).toEqual(['https://auth.example.com', true])
  expect(await set.findByDisplayValue('Authentik')).toBeTruthy()
  expect(set.getByDisplayValue('https://orbit.example.com/api/v1/auth/providers/oidc/callback')).toBeTruthy()
  expect(set.getByText('ORBIT_OIDC_*')).toBeTruthy()
  expect(set.queryByRole('button', { name: /Save (SSO|Authentik|OpenID)/ })).toBe(null)
  set.unmount()

  globalThis.fetch = (async () => Response.json({ registration_open: false, smtp: null, sign_in_providers: [], environment: ENVIRONMENT })) as unknown as typeof fetch
  const off = render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><AdminSignInProvidersPage /></QueryClientProvider>)
  expect(await off.findByText('ORBIT_OIDC_DISPLAY_NAME')).toBeTruthy()
  expect(off.queryByLabelText('Issuer')).toBe(null)
  expect(off.getByDisplayValue(`${window.location.origin}/api/v1/auth/providers/oidc/callback`)).toBeTruthy()
})
