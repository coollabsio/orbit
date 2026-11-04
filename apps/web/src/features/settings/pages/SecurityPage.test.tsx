import { afterEach, expect, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { registerConfirmationHandler } from '@/components/common/confirmAction'
import { SecurityPage } from './SecurityPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function problem(status: number, code: string, detail: string) {
  return Response.json({ type: 'about:blank', title: 'Request failed', status, code, detail, instance: '/', request_id: 'test' }, {
    status, headers: { 'content-type': 'application/problem+json' },
  })
}

const CODES = Array.from({ length: 10 }, (_, index) => `code${index}-abcde`)

type Account = { has_password?: boolean; providers?: string[]; connected?: { provider: string; email: string; created_at: string }[] }

/** `respond` answers a request first; anything it leaves (`undefined`) gets the default answer. */
function renderPage(initial: { totp_enabled: boolean; recovery_codes_left: number }, account: Account = {}, respond: (request: Request, path: string) => Response | undefined = () => undefined) {
  let status = initial
  const requests: string[] = []
  globalThis.fetch = (async (request: Request) => {
    const path = new URL(request.url).pathname
    requests.push(`${request.method} ${path}`)
    const answer = respond(request, path)
    if (answer) return answer
    if (path === '/api/v1/auth/options') return Response.json({ registration_open: false, email_enabled: false, passkeys_enabled: false, providers: account.providers ?? [], provider_registration_open: false })
    if (path === '/api/v1/auth/me') return Response.json({ id: 'user-1', email: 'ada@orbit.test', display_name: 'Ada', installation_admin: false, root: false, has_password: account.has_password ?? true, status: { presence: 'online' } })
    if (path === '/api/v1/auth/connected-accounts') return Response.json(account.connected ?? [])
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

test('disconnecting the only way to sign in shows why it was refused', async () => {
  const unregister = registerConfirmationHandler(async () => true)
  const { view, requests } = renderPage({ totp_enabled: false, recovery_codes_left: 0 }, {
    has_password: false,
    providers: ['google', 'github'],
    connected: [{ provider: 'google', email: 'ada@gmail.test', created_at: '2026-10-01T10:00:00Z' }],
  }, (request, path) => request.method === 'DELETE' && path === '/api/v1/auth/connected-accounts/google'
    ? problem(409, 'last_sign_in_method', 'Add a password or a passkey before you disconnect your last way to sign in.')
    : undefined)
  try {
    expect(await view.findByRole('button', { name: 'Connect GitHub' })).toBeTruthy()
    await userEvent.click(view.getByRole('button', { name: 'Disconnect Google' }))
    expect((await view.findByRole('alert')).textContent).toBe('Add a password or a passkey before you disconnect your last way to sign in.')
    expect(requests).toContain('DELETE /api/v1/auth/connected-accounts/google')
  } finally {
    unregister()
  }
})

test('an account without a password confirms with its provider when the server asks', async () => {
  const assign = spyOn(window.location, 'assign').mockImplementation(() => undefined)
  let start: unknown = null
  let setupBody: unknown = null
  const { view } = renderPage({ totp_enabled: false, recovery_codes_left: 0 }, {
    has_password: false,
    providers: ['github'],
    connected: [{ provider: 'github', email: 'ada', created_at: '2026-10-01T10:00:00Z' }],
  }, (request, path) => {
    if (path === '/api/v1/auth/two-factor/totp/setup') {
      setupBody = request.clone().json()
      return problem(401, 'reauthentication_required', 'Confirm it is you.')
    }
    if (path === '/api/v1/auth/providers/github/start') {
      start = request.clone().json()
      return Response.json({ authorize_url: 'https://github.com/login/oauth/authorize?client_id=c' })
    }
    return undefined
  })
  try {
    fireEvent.click(await view.findByRole('button', { name: 'Set up' }))
    await view.findByText('Orbit makes a key for your authenticator app.')
    expect(view.queryAllByLabelText('Current password')).toHaveLength(0)
    await userEvent.click(view.getByRole('button', { name: 'Continue' }))
    await userEvent.click(await view.findByRole('button', { name: 'Confirm with GitHub' }))
    await waitFor(() => expect(assign).toHaveBeenCalledWith('https://github.com/login/oauth/authorize?client_id=c'))
    expect(await setupBody).toEqual({ password: null })
    expect(await start).toMatchObject({ intent: 'confirm', redirect: 'web', return_to: '/profile/security' })
  } finally {
    assign.mockRestore()
  }
})

test('connecting an account confirms the password first and sends it with the start', async () => {
  const assign = spyOn(window.location, 'assign').mockImplementation(() => undefined)
  let start: unknown = null
  const { view } = renderPage({ totp_enabled: false, recovery_codes_left: 0 }, { providers: ['github'] }, (request, path) => {
    if (path !== '/api/v1/auth/providers/github/start') return undefined
    start = request.clone().json()
    return Response.json({ authorize_url: 'https://github.com/login/oauth/authorize?client_id=c' })
  })
  try {
    await userEvent.click(await view.findByRole('button', { name: 'Connect GitHub' }))
    await userEvent.type(await view.findByLabelText('Current password'), 'secret')
    await userEvent.click(view.getByRole('button', { name: 'Continue' }))
    await waitFor(() => expect(assign).toHaveBeenCalledWith('https://github.com/login/oauth/authorize?client_id=c'))
    expect(await start).toMatchObject({ intent: 'connect', redirect: 'web', return_to: '/profile/security', password: 'secret' })
  } finally {
    assign.mockRestore()
  }
})

test('a wrong password keeps the connect dialog open with the reason', async () => {
  const { view, requests } = renderPage({ totp_enabled: false, recovery_codes_left: 0 }, { providers: ['github'] }, (_request, path) =>
    path === '/api/v1/auth/providers/github/start' ? problem(401, 'invalid_credentials', 'The password is incorrect.') : undefined)
  await userEvent.click(await view.findByRole('button', { name: 'Connect GitHub' }))
  await userEvent.type(await view.findByLabelText('Current password'), 'wrong')
  await userEvent.click(view.getByRole('button', { name: 'Continue' }))
  expect(await view.findByText('The password is incorrect.')).toBeTruthy()
  expect(requests).toContain('POST /api/v1/auth/providers/github/start')
})

test('an account without a password connects straight away and confirms with its provider when the server asks', async () => {
  const starts: unknown[] = []
  const { view } = renderPage({ totp_enabled: false, recovery_codes_left: 0 }, {
    has_password: false,
    providers: ['google', 'github'],
    connected: [{ provider: 'google', email: 'ada@gmail.test', created_at: '2026-10-01T10:00:00Z' }],
  }, (request, path) => {
    if (path !== '/api/v1/auth/providers/github/start') return undefined
    starts.push(request.clone().json())
    return problem(401, 'reauthentication_required', 'Confirm it is you.')
  })
  await userEvent.click(await view.findByRole('button', { name: 'Connect GitHub' }))
  expect(await view.findByRole('button', { name: 'Confirm with Google' })).toBeTruthy()
  expect(view.queryAllByLabelText('Current password')).toHaveLength(0)
  expect(await Promise.all(starts)).toMatchObject([{ intent: 'connect', password: null }])
})
