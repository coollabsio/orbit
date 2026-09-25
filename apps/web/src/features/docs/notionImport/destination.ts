import type { NotionImport, PageSummary, Teamspace } from '@/api/generated/types.gen'
import { pageTitle } from '@/features/docs/pageTree'

/** Where an import's pages went, for the summary ("General", "Private", or "Inside “Roadmap”"). */
export function destinationLabel(
  destination: NotionImport['destination'],
  teamspaces: readonly Teamspace[] | undefined,
  pages: readonly PageSummary[] | undefined,
): string | null {
  if (!destination) return null
  if (destination.parent_page_id) {
    const page = pages?.find((item) => item.id === destination.parent_page_id)
    return page ? `Inside “${pageTitle(page)}”` : 'Inside a page'
  }
  if (destination.private) return 'Private'
  return teamspaces?.find((teamspace) => teamspace.id === destination.teamspace_id)?.name ?? 'Teamspace'
}
