/** The database reads and writes over the generated SDK. No React; the view logic is in `@orbit/database`. */
import { createDatabaseRow, getDatabase, updateDatabaseProperty, updateDatabaseRowValues, updateDatabaseView } from '@orbit/api/generated/sdk.gen'
import type { CreateDatabaseRowBody, DatabaseDetail, DatabaseRow, DatabaseView, DatabaseViewConfig } from '@orbit/api/generated/types.gen'
import type { SelectOption } from '@orbit/database/types'

import type { OrbitClient } from './orbit'

const databasePath = (workspaceId: string, databaseId: string) => ({ workspace_id: workspaceId, database_id: databaseId })

export async function database(client: OrbitClient, workspaceId: string, databaseId: string): Promise<DatabaseDetail> {
  return (await getDatabase({ client, path: databasePath(workspaceId, databaseId), throwOnError: true })).data
}

/** Values by property id; `null` clears one. */
export async function saveValues(client: OrbitClient, workspaceId: string, databaseId: string, rowId: string, values: Record<string, unknown>): Promise<DatabaseRow> {
  return (await updateDatabaseRowValues({ client, path: { ...databasePath(workspaceId, databaseId), row_id: rowId }, body: values, throwOnError: true })).data
}

export async function addRow(client: OrbitClient, workspaceId: string, databaseId: string, body: CreateDatabaseRowBody): Promise<DatabaseRow> {
  return (await createDatabaseRow({ client, path: databasePath(workspaceId, databaseId), body, throwOnError: true })).data
}

export async function saveView(client: OrbitClient, workspaceId: string, databaseId: string, viewId: string, config: DatabaseViewConfig): Promise<DatabaseView> {
  return (await updateDatabaseView({ client, path: { ...databasePath(workspaceId, databaseId), view_id: viewId }, body: { config }, throwOnError: true })).data
}

/** A select, multi-select or status property's whole option list, with the rest of its config kept. */
export async function saveOptions(client: OrbitClient, workspaceId: string, databaseId: string, propertyId: string, config: Record<string, unknown>, options: SelectOption[]): Promise<void> {
  await updateDatabaseProperty({ client, path: { ...databasePath(workspaceId, databaseId), property_id: propertyId }, body: { config: { ...config, options } }, throwOnError: true })
}
