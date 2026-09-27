import { afterEach, expect, spyOn, test } from 'bun:test'
import { toast } from 'sonner'
import { createApiClient } from '@/api/client'
import { PRINT_CLASS, dispositionFileName, downloadPageMarkdown, printPage } from './pageExport'

afterEach(() => {
  document.documentElement.classList.remove(PRINT_CLASS)
})

test('dispositionFileName prefers the UTF-8 name, falls back to the ASCII one, and never keeps paths', () => {
  expect(dispositionFileName(`attachment; filename="Caf_.zip"; filename*=UTF-8''Caf%C3%A9%20notes.zip`, 'x.zip')).toBe('Café notes.zip')
  expect(dispositionFileName('attachment; filename="Plain name.zip"', 'x.zip')).toBe('Plain name.zip')
  expect(dispositionFileName('attachment; filename=bare.zip', 'x.zip')).toBe('bare.zip')
  expect(dispositionFileName(`attachment; filename="ok.zip"; filename*=UTF-8''%E0%A4%A`, 'x.zip')).toBe('ok.zip')
  expect(dispositionFileName(`attachment; filename*=UTF-8''..%2F..%2Fevil.zip`, 'x.zip')).toBe('evil.zip')
  expect(dispositionFileName(null, 'Fallback.zip')).toBe('Fallback.zip')
})

function clientReturning(response: () => Response, seen: string[] = []) {
  return createApiClient({
    fetch: async (request) => {
      seen.push(`${request.method} ${new URL(request.url).pathname}${new URL(request.url).search}`)
      return response()
    },
  })
}

test('downloadPageMarkdown saves the ZIP under the server name and reports progress and success', async () => {
  const loading = spyOn(toast, 'loading')
  const success = spyOn(toast, 'success')
  const seen: string[] = []
  const saved: Array<{ name: string; size: number }> = []
  const client = clientReturning(
    () =>
      new Response(new Uint8Array([80, 75, 3, 4]), {
        headers: {
          'content-type': 'application/zip',
          'content-disposition': `attachment; filename="Plan.zip"; filename*=UTF-8''Pl%C3%A4n.zip`,
        },
      }),
    seen,
  )
  const ok = await downloadPageMarkdown({
    workspaceId: 'workspace-1',
    pageId: 'page-1',
    title: 'Plän',
    includeChildren: true,
    client,
    save: (blob, name) => saved.push({ name, size: blob.size }),
  })
  expect(ok).toBe(true)
  expect(seen).toEqual(['GET /api/v1/workspaces/workspace-1/pages/page-1/export?format=markdown&children=true'])
  expect(saved).toEqual([{ name: 'Plän.zip', size: 4 }])
  expect(loading.mock.calls.at(-1)?.[0]).toBe('Exporting “Plän” with sub-pages…')
  expect(success.mock.calls.at(-1)?.[0]).toBe('Exported “Plän”.')
  loading.mockRestore()
  success.mockRestore()
})

test('downloadPageMarkdown shows the limit message for 413 and a generic one otherwise', async () => {
  const failed = spyOn(toast, 'error')
  const problem = (status: number, code: string, detail: string) => () =>
    Response.json(
      { type: 'about:blank', title: 'Problem', status, code, detail, instance: '/x', request_id: 'r' },
      { status, headers: { 'content-type': 'application/problem+json' } },
    )
  const saved: string[] = []
  const run = (response: () => Response) =>
    downloadPageMarkdown({
      workspaceId: 'workspace-1',
      pageId: 'page-1',
      title: '',
      includeChildren: false,
      client: clientReturning(response),
      save: (_, name) => saved.push(name),
    })
  expect(await run(problem(413, 'export_too_large', 'Too big to export.'))).toBe(false)
  expect(failed.mock.calls.at(-1)?.[0]).toBe('Too big to export.')
  expect(await run(problem(404, 'page_not_found', 'gone'))).toBe(false)
  expect(failed.mock.calls.at(-1)?.[0]).toBe('“Untitled” is no longer available.')
  expect(await run(problem(500, 'internal_error', 'boom'))).toBe(false)
  expect(failed.mock.calls.at(-1)?.[0]).toBe('Could not export “Untitled”. Try again.')
  expect(saved).toEqual([])
  failed.mockRestore()
})

test('printPage prints with the print class and the page title, then restores both after printing', () => {
  const root = document.documentElement
  document.title = 'Orbit'
  const during: Array<{ printing: boolean; title: string }> = []
  let print = () => {
    during.push({ printing: root.classList.contains(PRINT_CLASS), title: document.title })
  }
  const events = new EventTarget()
  const win = {
    document,
    print: () => print(),
    addEventListener: events.addEventListener.bind(events),
    removeEventListener: events.removeEventListener.bind(events),
  } as unknown as Window
  printPage('Quarterly plan', win)
  expect(during).toEqual([{ printing: true, title: 'Quarterly plan' }])
  // Chrome's print() blocks until the dialog closes; afterprint follows.
  expect(root.classList.contains(PRINT_CLASS)).toBe(true)
  events.dispatchEvent(new Event('afterprint'))
  expect(root.classList.contains(PRINT_CLASS)).toBe(false)
  expect(document.title).toBe('Orbit')

  // A browser without print support leaves nothing behind.
  print = () => {
    throw new Error('not supported')
  }
  printPage('  ', win)
  expect(root.classList.contains(PRINT_CLASS)).toBe(false)
  expect(document.title).toBe('Orbit')
})

test('the print stylesheet is static CSS in index.css (no runtime <style> under the production CSP)', async () => {
  const css = await Bun.file(new URL('../../index.css', import.meta.url)).text()
  const print = css.slice(css.indexOf('@media print'))
  expect(print).toContain(`html.${PRINT_CLASS} body :not(:has([data-print-root])):not([data-print-root]):not([data-print-root] *)`)
  expect(print).toContain('[data-print-hide]')
  expect(print).toContain('color-scheme: light')
  expect(print).toContain('max-width: 100% !important')
})
