import { afterEach, expect, test } from 'bun:test'
import { act, screen } from '@testing-library/react'
import { Route, Routes } from 'react-router'
import type { CustomPageList } from '@orbit/api/generated/types.gen'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { CustomPageView } from './CustomPageView'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function serve(list: Pick<CustomPageList, 'items' | 'blocked'>) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = new URL(input instanceof Request ? input.url : String(input))
    if (url.pathname.endsWith('/custom-pages')) return Response.json({ can_manage: false, policy: { mode: 'any', origins: [] }, ...list })
    return new Response(null, { status: 404 })
  }) as typeof fetch
}

const page = {
  id: 'p1', workspace_id: 'w', slug: 'deploys', name: 'Deploys', icon: '🚀', admins_only: false, version: 0,
  url: 'https://grafana.example.com/d/abc?orgId=1', created_at: '', updated_at: '',
}

function open(path: string) {
  renderWithWorkspace(<Routes><Route path="/pages/:slug/*" element={<CustomPageView />} /></Routes>, { path })
}

test('a link below the page address opens that part of the page in a sandbox without top navigation', async () => {
  serve({ items: [page], blocked: [] })
  open('/pages/deploys/panel/2?from=now-1h')
  const frame = await screen.findByTitle('Deploys')
  const src = 'https://grafana.example.com/d/abc/panel/2?orgId=1&from=now-1h'
  expect(frame.getAttribute('src')).toBe(src)
  expect(frame.getAttribute('sandbox')?.split(' ')).not.toContain('allow-top-navigation')
  expect(frame.getAttribute('referrerpolicy')).toBe('no-referrer')
  expect(screen.getByRole('link', { name: /Open in new tab/ }).getAttribute('href')).toBe(src)
  // the host stays visible, so the frame cannot pass for Orbit
  expect(screen.getByText('grafana.example.com')).toBeTruthy()
})

test('a page whose origin the operator removed is not framed', async () => {
  serve({ items: [page], blocked: [page.id] })
  open('/pages/deploys')
  expect(await screen.findByText('Orbit cannot show this page here.')).toBeTruthy()
  expect(screen.queryByTitle('Deploys')).toBeNull()
  expect(screen.getByRole('link', { name: /Open in new tab/ }).getAttribute('href')).toBe(page.url)
})

test('an unknown address says so', async () => {
  serve({ items: [page], blocked: [] })
  open('/pages/other')
  expect(await screen.findByText('This page does not exist, or you cannot see it.')).toBeTruthy()
})

test('a site that the tab CSP still blocks asks for a reload', async () => {
  serve({ items: [page], blocked: [] })
  open('/pages/deploys')
  await screen.findByTitle('Deploys')
  // the origin was added after this tab loaded, so its CSP blocks the frame
  act(() => { document.dispatchEvent(Object.assign(new Event('securitypolicyviolation'), { effectiveDirective: 'frame-src', blockedURI: page.url })) })
  expect(await screen.findByText('Reload Orbit to show this page.')).toBeTruthy()
  expect(screen.queryByTitle('Deploys')).toBeNull()
})
