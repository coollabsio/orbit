import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SecurityPage } from './SecurityPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const CODES = Array.from({ length: 10 }, (_, index) => `code${index}-abcde`)

function renderPage(initial: { totp_enabled: boolean; recovery_codes_left: number }) {
  let status = initial
  const requests: string[] = []
  globalThis.fetch = (async (request: Request) => {
    const path = new URL(request.url).pathname
    requests.push(`${request.method} ${path}`)
    if (path === '/api/v1/auth/options') return Response.json({ registration_open: false, email_enabled: false, passkeys_enabled: false })
    if (path === '/api/v1/auth/two-factor') return Response.json(status)
    if (path === '/api/v1/auth/passkeys') return Response.json([])
    if (path === '/api/v1/auth/two-factor/totp/setup') {
      return Response.json({ secret: 'JBSWY3DPEHPK3PXP', otpauth_url: 'otpauth://totp/Orbit', qr_code: 'data:image/png;base64,AAAA' })
    }
    if (path === '/api/v1/auth/two-factor/totp/enable') {
      status = { totp_enabled: true, recovery_codes_left: 10 }
      return Response.json({ recovery_codes: CODES })
    }
    throw new Error('Unexpected request ' + path)
  }) as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><SecurityPage /></QueryClientProvider>)
  return { view, requests }
}

test('setting up the authenticator app ends with the recovery codes', async () => {
  const { view, requests } = renderPage({ totp_enabled: false, recovery_codes_left: 0 })
  expect(await view.findByText('Passkeys need Orbit to be opened at a domain name.')).toBeTruthy()
  expect(view.queryAllByRole('button', { name: 'Add passkey' })).toHaveLength(0)

  fireEvent.click(await view.findByRole('button', { name: 'Set up' }))
  await userEvent.type(await view.findByLabelText('Current password'), 'secret')
  await userEvent.click(view.getByRole('button', { name: 'Continue' }))

  const key = await view.findByLabelText('Setup key') as HTMLInputElement
  expect(key.value).toBe('JBSWY3DPEHPK3PXP')
  expect((view.getByAltText('QR code for your authenticator app') as HTMLImageElement).src).toBe('data:image/png;base64,AAAA')
  await userEvent.type(view.getByLabelText('Code from the app'), '123456')
  await userEvent.click(view.getByRole('button', { name: 'Turn on' }))

  const list = await view.findByRole('list', { name: 'Recovery codes' })
  expect(within(list).getAllByRole('listitem').map((item) => item.textContent)).toEqual(CODES)
  expect(requests).toContain('POST /api/v1/auth/two-factor/totp/enable')
  expect(await view.findByText('On · 10 recovery codes left')).toBeTruthy()
})

test('with the authenticator app on, the card offers new codes and turning it off', async () => {
  const { view } = renderPage({ totp_enabled: true, recovery_codes_left: 7 })
  expect(await view.findByText('On · 7 recovery codes left')).toBeTruthy()
  expect(view.getByRole('button', { name: 'Turn off' })).toBeTruthy()
  expect(view.getByRole('button', { name: 'New recovery codes' })).toBeTruthy()
  expect(view.queryAllByRole('button', { name: 'Set up' })).toHaveLength(0)
})
