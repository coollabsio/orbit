import { Link } from 'react-router'
import { Layer } from 'reicon-react'
import { EmptyState } from '@/components/common/EmptyState'
import { buttonVariants } from '@/components/ui/button'
import type { ViewStateController } from '../useViewState'
import { FavoriteStar } from './FavoriteStar'
import { PRESS_MOTION } from './motion'
import { ViewActionsMenu } from './ViewActionsMenu'
import { ViewIcon } from './ViewIcon'

export interface ViewHeaderProps {
  workspaceId: string
  controller: ViewStateController
  onEdit: () => void
  onDuplicate: () => void
  onDeleted: () => void
}

/** Replaces the project picker + title + spacer in the TasksPage header row on /views/:viewId. Unsaved edits are handled in the filter row (ViewChanges). */
export function ViewHeader({ workspaceId, controller, onEdit, onDuplicate, onDeleted }: ViewHeaderProps) {
  const { view } = controller
  if (!view) return null
  return (
    <>
      <div className="flex min-w-0 items-center gap-1.5 pl-1">
        <ViewIcon icon={view.icon} color={view.color} />
        <h1 className="truncate text-[13px] font-semibold text-foreground">{view.name}</h1>
        <FavoriteStar workspaceId={workspaceId} view={view} />
        <ViewActionsMenu workspaceId={workspaceId} view={view} onEdit={onEdit} onDuplicate={onDuplicate} onDeleted={onDeleted} />
      </div>
      <div className="flex-1" />
    </>
  )
}

/** Shown under the header when the stored state could not be parsed (spec § Errors). */
export function ViewStateBanner() {
  return (
    <div role="status" className="shrink-0 border-b border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
      This view has settings Orbit can't read. Update the view to reset them.
    </div>
  )
}

/** Missing or forbidden /views/:viewId (the server answers 404 for both). */
export function ViewNotFound() {
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background p-4 *:flex-1">
      <EmptyState
        icon={Layer}
        title="View not found"
        description="This view was deleted, or it isn't shared with you."
        action={<Link to="/views" className={buttonVariants({ variant: 'default', className: PRESS_MOTION })}>Go to views</Link>}
      />
    </div>
  )
}
