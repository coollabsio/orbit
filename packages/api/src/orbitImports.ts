/** UI helpers of the Orbit-to-Orbit importer (`/imports/orbit`) that both apps share. Calls go through `generated/sdk.gen`. */
import type { OrbitImport, OrbitImportSelection, OrbitImportTree } from './generated/types.gen'

const PROBLEM_MESSAGES: Record<string, string> = {
  app_key_missing: 'Orbit is not ready to import yet. Ask the Orbit administrator to set the app key.',
  orbit_url_invalid: 'Enter the address of the other Orbit instance, for example https://orbit.example.com.',
  orbit_token_invalid: 'The other Orbit instance rejected this token. Check that you copied the whole token and that it was not revoked.',
  orbit_scope_missing: 'This token cannot export. Create a token with the “Workspace export” or “Personal export” type.',
  orbit_unreachable: 'The other Orbit instance could not be reached. Check the address and try again.',
  orbit_export_too_large: 'The export is larger than this server accepts. Ask the administrator to raise the upload limit.',
  orbit_format_unsupported: 'This file is not an Orbit export, or it comes from a version this server cannot read.',
  import_in_progress: 'You already have an import running. Wait for it to finish or cancel it.',
  orbit_import_state_conflict: 'This import changed in the meantime (it may have expired or been cancelled).',
  forbidden: 'Only workspace owners and administrators can import a whole workspace. To import only your own private pages and chats, use a “Personal export” token.',
}

/** A user-facing sentence for a problem `code` from the Orbit importer, or null for codes it does not own. */
export function orbitImportProblemMessage(code: string): string | null {
  return PROBLEM_MESSAGES[code] ?? null
}

/** What the waiting step says while the import is `scanning`. */
export function orbitImportPhaseText(item: Pick<OrbitImport, 'phase'>): { headline: string; text: string } {
  switch (item.phase) {
    case 'exporting':
      return { headline: 'The other instance is exporting…', text: 'It builds a ZIP of everything the token can read. Large workspaces take a few minutes.' }
    case 'downloading':
      return { headline: 'Downloading the export…', text: 'Copying the ZIP from the other instance.' }
    default:
      return { headline: 'Reading the export…', text: 'Listing the projects, pages and channels it contains.' }
  }
}

export const CHANNEL_GROUPS: readonly { kind: 'public' | 'private' | 'dm'; label: string }[] = [
  { kind: 'public', label: 'Public channels' },
  { kind: 'private', label: 'Private channels' },
  { kind: 'dm', label: 'Direct messages' },
]

/** Everything in the tree, as the chooser starts. A personal import only sends private pages and channels. */
export function fullSelection(mode: OrbitImport['mode'], tree: OrbitImportTree): Required<OrbitImportSelection> {
  return {
    project_ids: mode === 'full' ? tree.projects.map((project) => project.id) : [],
    teamspace_ids: mode === 'full' ? tree.teamspaces.map((teamspace) => teamspace.id) : [],
    include_private_pages: tree.private_page_count > 0,
    channel_ids: tree.channels.map((channel) => channel.id),
  }
}

/** The name an import shows in lists and headers: the source workspace, else its address or "Orbit". */
export function orbitImportName(item: Pick<OrbitImport, 'source'>): string {
  return item.source.workspace_name || item.source.url || 'Orbit'
}
