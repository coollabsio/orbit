import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { ProjectGithubCard } from './ProjectGithubCard'
import { waitForAbsence } from '@/test/waitForAbsence'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}><MemoryRouter>{children}</MemoryRouter></QueryClientProvider>
}

test('project settings save an installed repository and label without an API token', async () => {
  let saved: unknown
  const settings = {
    app_slug: 'orbit-test', install_url: 'https://github.com/apps/orbit-test/installations/new',
    repository: null, label: null, repositories: [{ installation_id: 1234, repository: 'acme/repo' }],
    can_manage: true, key_configured: true,
  }
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PUT') {
      saved = await request.json()
      return Response.json({ ...settings, repository: 'acme/repo', label: 'Orbit' })
    }
    return Response.json(settings)
  }) as typeof fetch

  const view = render(<ProjectGithubCard workspaceId="workspace-1" projectId="project-1" />, { wrapper: Wrapper })
  const repository = await view.findByRole('combobox', { name: 'Repository' })
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
  const label = view.getByRole('textbox', { name: 'Issue label' })
  for (const style of ['rounded-lg', 'bg-transparent', 'dark:bg-input/30']) {
    expect(repository.classList.contains(style)).toBe(true)
    expect(label.classList.contains(style)).toBe(true)
  }
  fireEvent.click(repository)
  expect(view.getAllByRole('option').map((item) => item.textContent)).toEqual(['No repository', 'acme/repo'])
  await userEvent.click(await view.findByRole('option', { name: 'acme/repo' }))
  expect(repository.querySelector('[data-slot="select-value"]')?.textContent).toBe('acme/repo')
  expect(view.getByText('You have unsaved changes.')).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Save Changes' }))

  await waitFor(() => expect(saved).toEqual({ installation_id: 1234, repository: 'acme/repo', label: 'Orbit' }))
})

test('choosing No repository disconnects a linked project', async () => {
  let settings = {
    app_slug: 'orbit-test', install_url: null,
    repository: 'acme/repo' as string | null, label: 'Orbit' as string | null,
    repositories: [{ installation_id: 1234, repository: 'acme/repo' }],
    can_manage: true, key_configured: true,
  }
  let deleted = false
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'DELETE') {
      deleted = true
      settings = { ...settings, repository: null, label: null }
      return new Response(null, { status: 204 })
    }
    return Response.json(settings)
  }) as typeof fetch

  const view = render(<ProjectGithubCard workspaceId="workspace-1" projectId="project-1" />, { wrapper: Wrapper })
  const repository = await view.findByRole('combobox', { name: 'Repository' })
  fireEvent.click(repository)
  await userEvent.click(await view.findByRole('option', { name: 'No repository' }))
  expect(repository.querySelector('[data-slot="select-value"]')?.textContent).toBe('No repository')
  const save = view.getByRole('button', { name: 'Save Changes' })
  expect(save.hasAttribute('disabled')).toBe(false)
  fireEvent.click(save)
  await waitFor(() => expect(deleted).toBe(true))
  await waitForAbsence(() => view.queryByRole('button', { name: 'Save Changes' }))
})

test('Reset restores the saved repository and label without a request', async () => {
  let writes = 0
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    if ((input as Request).method !== 'GET') writes += 1
    return Response.json({
      app_slug: 'orbit-test', install_url: null, repository: 'acme/repo', label: 'Orbit',
      repositories: [{ installation_id: 1234, repository: 'acme/repo' }], can_manage: true, key_configured: true,
    })
  }) as typeof fetch

  const view = render(<ProjectGithubCard workspaceId="workspace-1" projectId="project-1" />, { wrapper: Wrapper })
  const label = await view.findByRole('textbox', { name: 'Issue label' }) as HTMLInputElement
  await userEvent.clear(label)
  await userEvent.type(label, 'New label')
  expect(view.getByRole('button', { name: 'Save Changes' })).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Reset' }))
  expect(label.value).toBe('Orbit')
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
  expect(writes).toBe(0)
})

test('project settings direct App setup to workspace settings', async () => {
  globalThis.fetch = (async () => Response.json({ app_slug: null, install_url: null, repository: null, label: null, repositories: [], can_manage: true, key_configured: true })) as unknown as typeof fetch
  const view = render(<ProjectGithubCard workspaceId="workspace-1" projectId="project-1" />, { wrapper: Wrapper })
  const link = await view.findByRole('link', { name: 'workspace settings' })
  expect(link.getAttribute('href')).toBe('/settings/github?workspace=workspace-1')
  expect(view.queryByRole('button', { name: 'Register GitHub App' })).toBeNull()
})
