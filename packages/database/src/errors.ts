import { ApiProblem } from '@orbit/api/problem'

/** The message for a failed database write: the lock, the row cap and a conflict have their own words. */
export function databaseErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof ApiProblem) {
    if (error.code === 'page_locked') return 'This database is locked. Unlock it to change its properties and views.'
    if (error.code === 'database_row_limit') return 'This database has the most rows it can hold (5,000).'
    if (error.code === 'database_last_view') return 'A database needs at least one view.'
    if (error.code === 'database_button_rate_limit') return 'Too many button presses. Try again in a moment.'
    if (error.code === 'conflict') return 'This row changed somewhere else. The table now shows the latest version.'
    if (error.status === 422) return error.detail
  }
  return fallback
}
