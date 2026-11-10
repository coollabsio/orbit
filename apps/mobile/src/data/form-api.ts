/** The form view's submit request over the generated SDK (spec P3 18.2). */
import { submitDatabaseForm } from '@orbit/api/generated/sdk.gen'
import type { DatabaseRow } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { databaseErrorMessage } from '@orbit/database/errors'

import type { OrbitClient } from './orbit'

/** Answers by property id in the API's value shape; the server adds the row as the caller. */
export async function submitForm(client: OrbitClient, workspaceId: string, databaseId: string, viewId: string, answers: Record<string, unknown>): Promise<DatabaseRow> {
  return (await submitDatabaseForm({ client, path: { workspace_id: workspaceId, database_id: databaseId, view_id: viewId }, body: { answers }, throwOnError: true })).data
}

/** The words for a failed submission, as on the web. */
export function formErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    if (error.code === 'database_form_closed') return 'This form no longer takes answers.'
    if (error.code === 'database_form_rate_limited') return 'You sent this form too often. Try again in an hour.'
    if (error.code === 'database_view_not_found') return 'This form was deleted.'
  }
  return databaseErrorMessage(error, 'Could not send the form. Try again.')
}

/** The native form screen of a web form link on this server (`<server>/docs/<database>/form/<view>`, with a linked view block's `?link=`), else `null`. */
export function formScreenOf(url: string, server: string): string | null {
  try {
    const target = new URL(url)
    if (target.origin !== new URL(server).origin) return null
    const match = /^\/docs\/([^/]+)\/form\/([^/]+)\/?$/.exec(target.pathname)
    if (!match) return null
    const link = target.searchParams.get('link')
    return `/docs/form/${match[1]}/${match[2]}${link ? `?link=${encodeURIComponent(link)}` : ''}`
  } catch {
    return null
  }
}
