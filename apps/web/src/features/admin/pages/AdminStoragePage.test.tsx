import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { EnvironmentView } from '@orbit/api/generated/types.gen'
import { AdminStoragePage } from './AdminStoragePage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const NO_BUCKET = { s3: null, attachments_in_s3: false, backups_in_s3: false, backup_schedule: 'off', files_to_move: 0, move_error: null }
const S3 = { endpoint: 'http://minio:9000', region: 'us-east-1', bucket: 'orbit-files', prefix: '', access_key_id: 'orbit', path_style: true }
const ENVIRONMENT: EnvironmentView = { registration_open: false, smtp: false, s3: false, attachments_in_s3: false, backups_in_s3: false, backup_schedule: false, sign_in_providers: [] }

test('a bucket is saved first, then a switch sends the other options unchanged', async () => {
  const requests: { url: string; body: unknown }[] = []
  let storage: object = NO_BUCKET
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PUT') {
      const body = await request.json()
      requests.push({ url: new URL(request.url).pathname, body })
      storage = request.url.endsWith('/s3')
        ? { ...NO_BUCKET, s3: S3, backup_schedule: 'daily' }
        : { ...NO_BUCKET, s3: S3, ...(body as object) }
    }
    return Response.json({ registration_open: false, smtp: null, storage, environment: ENVIRONMENT })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminStoragePage /></QueryClientProvider>)

  const attachments = await view.findByRole('switch', { name: 'Attachments in S3' })
  expect(attachments.getAttribute('aria-disabled') === 'true' || attachments.hasAttribute('data-disabled')).toBe(true)

  await userEvent.type(view.getByLabelText(/^Endpoint/), 'http://minio:9000')
  await userEvent.type(view.getByLabelText(/^Bucket/), 'orbit-files')
  await userEvent.type(view.getByLabelText(/^Access key ID/), 'orbit')
  await userEvent.type(view.getByLabelText(/^Secret access key/), 'secret')
  await userEvent.click(view.getByRole('button', { name: 'Save bucket' }))
  await waitFor(() => expect(requests).toHaveLength(1))
  expect(requests[0]).toEqual({
    url: '/api/v1/admin/settings/s3',
    body: { endpoint: 'http://minio:9000', region: null, bucket: 'orbit-files', prefix: null, access_key_id: 'orbit', secret_access_key: 'secret', path_style: false },
  })

  await waitFor(() => expect(view.getByRole('switch', { name: 'Attachments in S3' }).hasAttribute('data-disabled')).toBe(false))
  fireEvent.click(view.getByRole('switch', { name: 'Attachments in S3' }))
  await waitFor(() => expect(requests).toHaveLength(2))
  expect(requests[1]).toEqual({
    url: '/api/v1/admin/settings/storage',
    body: { attachments_in_s3: true, backups_in_s3: false, backup_schedule: 'daily' },
  })
})

test('settings set by the environment are read-only', async () => {
  const storage = { ...NO_BUCKET, s3: S3, attachments_in_s3: true, backup_schedule: 'daily' }
  const environment = { ...ENVIRONMENT, s3: true, attachments_in_s3: true, backup_schedule: true }
  globalThis.fetch = (async () => Response.json({ registration_open: false, smtp: null, storage, environment })) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminStoragePage /></QueryClientProvider>)

  const endpoint = await view.findByLabelText(/^Endpoint/)
  expect((endpoint as HTMLInputElement).disabled).toBe(true)
  expect((view.getByRole('button', { name: 'Save bucket' }) as HTMLButtonElement).disabled).toBe(true)
  expect((view.getByRole('button', { name: 'Remove' }) as HTMLButtonElement).disabled).toBe(true)
  const attachments = view.getByRole('switch', { name: 'Attachments in S3' })
  expect(attachments.getAttribute('aria-disabled') === 'true' || attachments.hasAttribute('data-disabled')).toBe(true)
  const backups = view.getByRole('switch', { name: 'Backups in S3' })
  expect(backups.getAttribute('aria-disabled') === 'true' || backups.hasAttribute('data-disabled')).toBe(false)
  expect((view.getByRole('combobox', { name: 'Automatic backups' }) as HTMLButtonElement).disabled).toBe(true)
  expect(view.getByText('ORBIT_S3_*')).toBeTruthy()
})
