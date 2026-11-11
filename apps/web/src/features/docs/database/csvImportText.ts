import type { DatabaseImportResult } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import type { CsvTable } from '@orbit/database/csv'

/** The words for a failed import: a 422 names the row or column it is about. */
export function csvImportErrorMessage(error: unknown, table: CsvTable): string {
  if (error instanceof ApiProblem) {
    if (error.code === 'database_row_limit') return 'The rows do not fit: a database holds at most 5,000 rows.'
    if (error.code === 'database_property_limit') return 'The new properties do not fit: a database holds at most 100 properties.'
    if (error.code === 'page_locked') return 'This database is locked. Unlock it to add properties, or map every column to an existing property or Skip.'
    if (error.status === 413) return 'The file is too large to import (5 MB at most).'
    if (error.status === 422) {
      const row = /^rows\[(\d+)\]/.exec(error.detail)
      if (row) return `Row ${Number(row[1]) + 1} has more cells than the file has columns.`
      const column = /^columns\[(\d+)\]/.exec(error.detail)
      if (column) return `Column “${table.header[Number(column[1])] ?? ''}” cannot be imported this way. Choose another property for it.`
      if (error.detail === 'columns') return 'Map one column to the title, and each property to one column at most.'
      return error.detail
    }
  }
  return 'Could not import the CSV. Try again.'
}

/** What an import did, in words ("Imported 12 rows. 3 new options."). */
export function importSummary(result: DatabaseImportResult): string {
  const parts = [`Imported ${result.created_rows.toLocaleString()} ${result.created_rows === 1 ? 'row' : 'rows'}.`]
  if (result.created_options > 0) parts.push(`${result.created_options.toLocaleString()} new ${result.created_options === 1 ? 'option' : 'options'}.`)
  if (result.cleared > 0) parts.push(`${result.cleared.toLocaleString()} ${result.cleared === 1 ? 'value' : 'values'} left empty.`)
  if (result.values_truncated > 0) parts.push(`${result.values_truncated.toLocaleString()} long ${result.values_truncated === 1 ? 'value was' : 'values were'} cut.`)
  return parts.join(' ')
}
