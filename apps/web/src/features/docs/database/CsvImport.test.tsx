import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { DatabaseImportResult } from '@orbit/api/generated/types.gen'
import { parseProperty } from '@orbit/database/values'
import { MergeCsvDialog } from './CsvImport'
import { importSummary } from './csvImportText'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const properties = [
  parseProperty({ id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 }),
  parseProperty({ id: 'p-score', name: 'Score', type: 'number', config: { format: 'number' }, position: 1 }),
]

const counts = (converted: number, cleared: number) => ({ converted, cleared, created_options: 0, values_truncated: 0 })
const result: DatabaseImportResult = {
  created_rows: 2,
  converted: 3,
  cleared: 1,
  created_options: 0,
  values_truncated: 0,
  columns: [counts(2, 0), counts(1, 1)],
}

test('merge maps columns by name, confirms values that will be cleared, then imports', async () => {
  const bodies: Record<string, unknown>[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    bodies.push({ path: new URL(request.url).pathname, ...(JSON.parse(await request.text()) as object) })
    return Response.json(result)
  }) as unknown as typeof fetch
  let merged: DatabaseImportResult | null = null
  const client = new QueryClient()
  render(
    <QueryClientProvider client={client}>
      <MergeCsvDialog workspaceId="ws" databaseId="db" properties={properties} canAddProperties onClose={() => {}} onMerged={(value) => (merged = value)} />
    </QueryClientProvider>,
  )
  const file = new File(['name,score\nPie,10\nTart,lots\n'], 'pies.csv', { type: 'text/csv' })
  fireEvent.change(screen.getByLabelText('CSV file'), { target: { files: [file] } })
  await screen.findByText('pies.csv')
  expect(screen.getByRole('table', { name: 'Preview' }).textContent).toContain('Tart')

  fireEvent.click(screen.getByRole('button', { name: 'Import 2 rows' }))
  await screen.findByText('1 value does not fit its property and will be left empty')
  expect(screen.getByText('1 value in “score”')).toBeDefined()
  fireEvent.click(screen.getByRole('button', { name: 'Import anyway' }))
  await waitFor(() => expect(merged).toEqual(result))

  const columns = [
    { name: 'name', target: { property_id: 'p-title' } },
    { name: 'score', target: { property_id: 'p-score' } },
  ]
  const rows = [
    ['Pie', '10'],
    ['Tart', 'lots'],
  ]
  expect(bodies).toEqual([
    { path: '/api/v1/workspaces/ws/databases/db/import', columns, rows, dry_run: true },
    { path: '/api/v1/workspaces/ws/databases/db/import', columns, rows, dry_run: false },
  ])
  expect(importSummary(result)).toBe('Imported 2 rows. 1 value left empty.')
})
