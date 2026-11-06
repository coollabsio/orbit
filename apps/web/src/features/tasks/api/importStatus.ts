import type { AsanaImportStatus, LinearImportStatus, TrelloImportStatus } from '@orbit/api/generated/types.gen'

/** The states every task import (Asana, Linear, Trello) goes through. */
export type TaskImportStatus = AsanaImportStatus | LinearImportStatus | TrelloImportStatus

/** How often an import that is still working is polled. */
export const IMPORT_POLL_MS = 2000

/** A job is working on it: poll. */
export function isActiveImport(status: TaskImportStatus | undefined): boolean {
  return status === 'scanning' || status === 'queued' || status === 'importing'
}

/** Nothing will change any more. */
export function isFinalImport(status: TaskImportStatus | undefined): boolean {
  return status === 'completed' || status === 'failed' || status === 'cancelled' || status === 'expired'
}

export const IMPORT_STATUS_LABELS: Record<TaskImportStatus, string> = {
  scanning: 'Scanning',
  ready: 'Ready to import',
  queued: 'Queued',
  importing: 'Importing',
  completed: 'Completed',
  failed: 'Failed',
  cancelled: 'Cancelled',
  expired: 'Expired',
}

/** The badge variant of an import status. */
export function importStatusVariant(status: TaskImportStatus): 'default' | 'secondary' | 'destructive' | 'outline' {
  if (status === 'failed') return 'destructive'
  if (status === 'completed') return 'secondary'
  if (isActiveImport(status) || status === 'ready') return 'default'
  return 'outline'
}
