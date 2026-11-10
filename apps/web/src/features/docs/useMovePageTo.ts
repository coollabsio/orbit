import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { movePage } from '@orbit/api/generated/sdk.gen'
import type { PageSummary, Teamspace } from '@orbit/api/generated/types.gen'
import { droppedValuesText } from '@orbit/database/moves'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { confirmAction } from '@/components/common/confirmAction'
import { isPageVersionConflict, movePageErrorMessage, useMovePage } from '@/features/docs/api/pages'
import { childrenOf, pageTitle, rootsOf, spaceLabel, spaceRequest, type MoveTarget, type SpaceKey } from '@/features/docs/pageTree'

/** Where "Move to" sends a page: the last root page of a space, or the last sub-page (or row) of a page or database. */
export type MoveDestination = { kind: 'space'; space: SpaceKey } | { kind: 'page'; page: PageSummary }

export interface MovingPage {
  id: string
  version: number
  /** Set for a row: leaving its database deletes values, so the move asks first. */
  database_id: string | null
}

function destinationMove(pages: readonly PageSummary[], moving: MovingPage, destination: MoveDestination): MoveTarget {
  if (destination.kind === 'page') {
    const siblings = childrenOf(pages, destination.page.id).filter((page) => page.id !== moving.id)
    return { parent_id: destination.page.id, position: siblings.length }
  }
  const roots = rootsOf(pages, destination.space).filter((page) => page.id !== moving.id)
  return { parent_id: null, position: roots.length, ...spaceRequest(destination.space) }
}

/**
 * "Move to" for one page. A row that leaves its database first asks the server how many of its values the move
 * deletes (a dry run) and confirms when there are any. `flush` saves the open editor and returns the page's version.
 * Resolves false when nothing is sent (cancelled, or the dry run failed).
 */
export function useMovePageTo(workspaceId: string, pages: readonly PageSummary[], teamspaces: readonly Teamspace[] | undefined) {
  const queryClient = useQueryClient()
  const move = useMovePage(workspaceId)
  return async (moving: MovingPage, destination: MoveDestination, flush?: () => Promise<number>): Promise<boolean> => {
    const target = destinationMove(pages, moving, destination)
    const version = flush ? await flush() : moving.version
    const from = moving.database_id
    if (from !== null && target.parent_id !== from) {
      let dropped: number
      try {
        const { data } = await movePage({
          client: apiClient,
          path: { workspace_id: workspaceId, page_id: moving.id },
          body: { expected_version: version, ...target, dry_run: true },
          throwOnError: true,
        })
        dropped = data.dropped_values
      } catch (error) {
        toast.error(movePageErrorMessage(error))
        return false
      }
      if (
        dropped > 0 &&
        !(await confirmAction({ title: 'Move this row out of its database?', description: droppedValuesText(dropped), confirmLabel: 'Move', danger: true }))
      ) {
        return false
      }
    }
    const label = destination.kind === 'space' ? spaceLabel(destination.space, teamspaces) : `“${pageTitle(destination.page)}”`
    move.mutate(
      { pageId: moving.id, version, ...target },
      {
        onSuccess: () => {
          toast.success(`Moved to ${label}`)
          if (from !== null) void queryClient.invalidateQueries({ queryKey: queryKeys.database(workspaceId, from) })
        },
        onError: (error) => {
          if (!isPageVersionConflict(error)) toast.error(movePageErrorMessage(error))
        },
      },
    )
    return true
  }
}
