import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Layer } from 'reicon-react'
import { toast } from 'sonner'
import { ApiProblem } from '@/api/problem'
import { EmptyState } from '@/components/common/EmptyState'
import { Button, buttonVariants } from '@/components/ui/button'
import type { ViewStateController } from '../useViewState'
import { ConflictDialog } from './ConflictDialog'
import { FavoriteStar } from './FavoriteStar'
import { PRESS_MOTION } from './motion'
import { ViewActionsMenu } from './ViewActionsMenu'
import { ViewIcon } from './ViewIcon'

export interface ViewHeaderProps {
  workspaceId: string
  controller: ViewStateController
  onSaveAsNew: () => void
  onEdit: () => void
  onDuplicate: () => void
  onDeleted: () => void
}

/** Replaces the project picker + title + spacer in the TasksPage header row on /views/:viewId. */
export function ViewHeader({ workspaceId, controller, onSaveAsNew, onEdit, onDuplicate, onDeleted }: ViewHeaderProps) {
  const [saving, setSaving] = useState(false)
  const [conflictDismissed, setConflictDismissed] = useState(false)
  const { view, dirty, canEdit } = controller

  const save = async (overwrite = false) => {
    setConflictDismissed(false)
    setSaving(true)
    try {
      // a 409 resolves and sets `controller.conflict`, which opens the conflict dialog with the edits kept
      await controller.save(overwrite ? { overwrite: true } : undefined)
    } catch (error) {
      toast.error(error instanceof ApiProblem ? error.detail : "Orbit couldn't save the view. Try again.")
    } finally {
      setSaving(false)
    }
  }

  // Cmd/Ctrl+S works even while typing; it only acts when there is something to save (and never animates)
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.key.toLowerCase() !== 's' || !dirty) return
      event.preventDefault()
      if (canEdit) void save()
      else onSaveAsNew()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  })

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
      {dirty ? (
        <div role="group" aria-label="Unsaved view changes" className="flex animate-view-bar-enter items-center gap-1">
          <span className="mr-1 text-xs text-muted-foreground max-[899px]:hidden">Unsaved changes</span>
          <Button type="button" variant="ghost" size="sm" className={PRESS_MOTION} onClick={() => controller.discard()}>Discard</Button>
          {canEdit ? (
            <Button type="button" size="sm" className={PRESS_MOTION} disabled={saving} aria-keyshortcuts="Control+S Meta+S" onClick={() => void save()}>Save</Button>
          ) : (
            <Button type="button" size="sm" className={PRESS_MOTION} aria-keyshortcuts="Control+S Meta+S" onClick={onSaveAsNew}>Save as new view</Button>
          )}
          <span aria-hidden="true" className="mx-1 h-4 w-px bg-border" />
        </div>
      ) : null}
      <ConflictDialog
        open={controller.conflict && !conflictDismissed}
        pending={saving}
        onOpenChange={(open) => {
          if (!open) setConflictDismissed(true)
        }}
        // after a conflict, discard() drops the edits and refetches the view: it is Reload
        onReload={() => controller.discard()}
        onOverwrite={() => void save(true)}
      />
    </>
  )
}

/** Shown under the header when the stored state could not be parsed (spec § Errors). */
export function ViewStateBanner() {
  return (
    <div role="status" className="shrink-0 border-b border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
      This view has settings Orbit can't read. Save to reset them.
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
