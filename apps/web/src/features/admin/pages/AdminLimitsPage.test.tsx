import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { EnvironmentView } from '@orbit/api/generated/types.gen'
import { AdminLimitsPage } from './AdminLimitsPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const STORAGE = { s3: null, attachments_in_s3: false, backups_in_s3: false, backup_schedule: 'off', files_to_move: 0, move_error: null }
const LIMITS = { max_owned_workspaces: 10, workspace_storage_quota_bytes: 5_000_000_000, image_proxy_fetches: 32 }
const ENVIRONMENT: EnvironmentView = { registration_open: false, oidc_provision: false, smtp: false, s3: false, attachments_in_s3: false, backups_in_s3: false, backup_schedule: false, s3_keep_deleted_days: false, image_proxy_fetches: false, sign_in_providers: [] }

test('limits are saved in bytes, and an empty field is no limit', async () => {
  const bodies: unknown[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PUT') bodies.push({ url: new URL(request.url).pathname, body: await request.json() })
    return Response.json({ registration_open: false, smtp: null, storage: STORAGE, limits: LIMITS, environment: ENVIRONMENT })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminLimitsPage /></QueryClientProvider>)

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

function serve(environment: EnvironmentView, bodies: unknown[]) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PUT') bodies.push(await request.json())
    return Response.json({ registration_open: false, smtp: null, storage: STORAGE, limits: LIMITS, environment })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><AdminLimitsPage /></QueryClientProvider>)
}

test('the image limit is saved with the other limits unchanged, and only from 1 to 256', async () => {
  const bodies: unknown[] = []
  const view = serve(ENVIRONMENT, bodies)

  const fetches = await view.findByLabelText('Images loaded at once per server')
  expect((fetches as HTMLInputElement).value).toBe('32')
  await userEvent.clear(fetches)
  await userEvent.type(fetches, '300')
  await userEvent.click(view.getByRole('button', { name: 'Save image limit' }))
  await userEvent.clear(fetches)
  await userEvent.type(fetches, '8')
  await userEvent.click(view.getByRole('button', { name: 'Save image limit' }))
  await waitFor(() => expect(bodies).toHaveLength(1))
  expect(bodies[0]).toEqual({ max_owned_workspaces: 10, workspace_storage_quota_bytes: 5_000_000_000, image_proxy_fetches: 8 })
})

test('an image limit that the environment sets cannot be changed here', async () => {
  const view = serve({ ...ENVIRONMENT, image_proxy_fetches: true }, [])

  const fetches = await view.findByLabelText('Images loaded at once per server')
  expect((fetches as HTMLInputElement).disabled).toBe(true)
  expect(view.getByText('ORBIT_IMAGE_PROXY_FETCHES')).toBeTruthy()
  expect(view.queryByRole('button', { name: 'Save image limit' })).toBeNull()
})
