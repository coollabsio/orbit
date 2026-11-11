import { toast } from 'sonner'
import { viewCsv, viewCsvName, viewProperties } from '@orbit/database/csv'
import type { Database, DatabaseView, Row, ViewContext } from '@orbit/database/types'
import { saveBlob } from '@/features/docs/pageExport'

/** "⋯ → Export view as CSV": the rows the view shows (filter, search and sort) under its visible properties. */
export function downloadViewCsv(database: Database, view: DatabaseView, rows: readonly Row[], ctx: ViewContext, save = saveBlob): void {
  const text = viewCsv(viewProperties(database, view), rows, ctx)
  save(new Blob([text], { type: 'text/csv;charset=utf-8' }), viewCsvName(database, view))
  toast.success(`Exported ${rows.length.toLocaleString()} ${rows.length === 1 ? 'row' : 'rows'}.`)
}
