import { afterEach, expect, test } from 'bun:test'
import type { WorkspaceRole } from '@orbit/api/generated/types.gen'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { ConnectGithubPrompt } from './ConnectGithubPrompt'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function show(status: string, role: WorkspaceRole = 'owner') {
  globalThis.fetch = (async () => Response.json({ status, login: status === 'broken' ? 'octo' : null })) as unknown as typeof fetch
  return renderWithWorkspace(<ConnectGithubPrompt workspaceId="workspace-1" />, { role })
}

test('a member without a connection is asked to connect', async () => {
  const view = show('disconnected', 'member')
  expect(await view.findByText('Connect GitHub to see this pull request.')).toBeTruthy()
  expect(view.getByRole('button', { name: 'Connect GitHub' })).toBeTruthy()
})

test('a broken connection asks to reconnect', async () => {
  const view = show('broken', 'member')
  expect(await view.findByRole('button', { name: 'Reconnect GitHub' })).toBeTruthy()
})

test('an old app sends managers to the GitHub settings', async () => {
  const view = show('needs_recreate', 'owner')
  expect(await view.findByText(/re-create the GitHub App/)).toBeTruthy()
  expect(view.getByRole('link', { name: 'Open GitHub settings' }).getAttribute('href')).toBe('/settings/github')
})

test('an old app asks members to contact an admin', async () => {
  const view = show('needs_recreate', 'member')
  expect(await view.findByText(/Ask a workspace owner or admin/)).toBeTruthy()
  expect(view.queryAllByRole('link', { name: 'Open GitHub settings' }).length).toBe(0)
})

test('no app at all asks for one', async () => {
  const view = show('unavailable', 'member')
  expect(await view.findByText(/This workspace has no GitHub App/)).toBeTruthy()
})
