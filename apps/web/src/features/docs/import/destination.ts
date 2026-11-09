import { useMemo, useState } from 'react'
import type { NotionImportDestination, PageSummary, Teamspace } from '@orbit/api/generated/types.gen'
import { PRIVATE_SPACE, childrenOf, defaultTeamspace, pageTitle, rootsOf, spaceRequest, teamspaceSpace, type SpaceKey } from '@/features/docs/pageTree'

/** Where an import's pages went, for the summary ("General", "Private", or "Inside “Roadmap”"). */
export function destinationLabel(
  destination: NotionImportDestination | null,
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

/** The "Inside page" value for the space's top level. */
export const TOP_LEVEL = '__top__'

/** The `destination` body of a Docs import's `start`. */
export type ImportDestinationBody = { parent_page_id: string } | ReturnType<typeof spaceRequest>

/** Pages of one space in tree order, with their depth (the "Inside page" options). */
function spacePages(pages: readonly PageSummary[], space: SpaceKey): { page: PageSummary; depth: number }[] {
  const out: { page: PageSummary; depth: number }[] = []
  const walk = (list: PageSummary[], depth: number) => {
    for (const page of list) {
      out.push({ page, depth })
      walk(childrenOf(pages, page.id), depth + 1)
    }
  }
  walk(rootsOf(pages, space), 0)
  return out
}

/** Where a Docs import puts its top-level pages: a space (the default teamspace first), optionally inside a page. */
export function useImportDestination(teamspaces: Teamspace[] | undefined, pages: PageSummary[] | undefined) {
  const [chosenSpace, setChosenSpace] = useState<SpaceKey | null>(null)
  const [parentId, setParentId] = useState(TOP_LEVEL)
  const fallbackTeamspace = defaultTeamspace(teamspaces)
  const space: SpaceKey = chosenSpace ?? (fallbackTeamspace ? teamspaceSpace(fallbackTeamspace.id) : PRIVATE_SPACE)
  const parentOptions = useMemo(() => spacePages(pages ?? [], space), [pages, space])
  // A page that moved away (or was trashed) is no longer a valid choice.
  const parent = parentId !== TOP_LEVEL && parentOptions.some(({ page }) => page.id === parentId) ? parentId : TOP_LEVEL
  const body: ImportDestinationBody = parent !== TOP_LEVEL ? { parent_page_id: parent } : spaceRequest(space)
  return {
    teamspaces,
    space,
    parent,
    parentOptions,
    body,
    chooseSpace: (next: SpaceKey) => {
      setChosenSpace(next)
      setParentId(TOP_LEVEL)
    },
    chooseParent: (next: string | null) => setParentId(next ?? TOP_LEVEL),
  }
}
