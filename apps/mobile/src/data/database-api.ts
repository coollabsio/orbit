/** The database reads and writes over the generated SDK. No React; the view logic is in `@orbit/database`. */
import { createDatabaseRow, getDatabase, pressDatabaseButton, updateDatabaseProperty, updateDatabaseRowValues, updateDatabaseView } from '@orbit/api/generated/sdk.gen'
import type { CreateDatabaseRowBody, DatabaseButtonPress, DatabaseDetail, DatabaseRow, DatabaseView, DatabaseViewConfig } from '@orbit/api/generated/types.gen'
import type { SelectOption } from '@orbit/database/types'

import type { OrbitClient } from './orbit'

const databasePath = (workspaceId: string, databaseId: string) => ({ workspace_id: workspaceId, database_id: databaseId })

/** With `linkId`, `views` are the linked view block's instead of the database's own. Formulas compute in the viewer's zone. */
export async function database(client: OrbitClient, workspaceId: string, databaseId: string, linkId = ''): Promise<DatabaseDetail> {
  const tz = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
  return (await getDatabase({ client, path: databasePath(workspaceId, databaseId), query: { tz, ...(linkId ? { link: linkId } : {}) }, throwOnError: true })).data
}

/** Values by property id; `null` clears one. */
export async function saveValues(client: OrbitClient, workspaceId: string, databaseId: string, rowId: string, values: Record<string, unknown>): Promise<DatabaseRow> {
  return (await updateDatabaseRowValues({ client, path: { ...databasePath(workspaceId, databaseId), row_id: rowId }, body: values, throwOnError: true })).data
}

/** Runs a button's actions; `today` is the phone's day (`YYYY-MM-DD`). */
export async function pressButton(client: OrbitClient, workspaceId: string, databaseId: string, rowId: string, propertyId: string, today: string): Promise<DatabaseButtonPress> {
  return (await pressDatabaseButton({ client, path: { ...databasePath(workspaceId, databaseId), row_id: rowId, property_id: propertyId }, body: { today }, throwOnError: true })).data
}

export async function addRow(client: OrbitClient, workspaceId: string, databaseId: string, body: CreateDatabaseRowBody): Promise<DatabaseRow> {
  return (await createDatabaseRow({ client, path: databasePath(workspaceId, databaseId), body, throwOnError: true })).data
}

export async function saveView(client: OrbitClient, workspaceId: string, databaseId: string, linkId: string, viewId: string, config: DatabaseViewConfig): Promise<DatabaseView> {
  const query = linkId ? { link_id: linkId } : undefined
  return (await updateDatabaseView({ client, path: { ...databasePath(workspaceId, databaseId), view_id: viewId }, query, body: { config }, throwOnError: true })).data
}

/** A select, multi-select or status property's whole option list, with the rest of its config kept. */
export async function saveOptions(client: OrbitClient, workspaceId: string, databaseId: string, propertyId: string, config: Record<string, unknown>, options: SelectOption[]): Promise<void> {
  await updateDatabaseProperty({ client, path: { ...databasePath(workspaceId, databaseId), property_id: propertyId }, body: { config: { ...config, options } }, throwOnError: true })
}
