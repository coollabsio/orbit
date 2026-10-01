import { useCommand, useShortcutLabel } from '@/shortcuts/useCommand'
import { useRef, useState } from 'react'
import { Bookmark } from 'reicon-react'
import { toast } from 'sonner'
import { ApiProblem } from '@/api/problem'
import { Button } from '@/components/ui/button'
import type { ViewStateController } from '../useViewState'
import { ConflictDialog } from './ConflictDialog'

export interface ViewChangesProps {
  controller: ViewStateController
  /** `instant` when opened from the keyboard (Cmd/Ctrl+S): the dialog skips its entrance animation. */
  onSaveAsNew: (options?: { instant?: boolean }) => void
}

/**
 * Reset · Save as new view · Update view, at the end of the filter row of a saved view that has unsaved edits.
 * Mounted only while the view is dirty. Cmd/Ctrl+S updates the view (or, for non-editors, saves it as new).
 */
export function ViewChanges({ controller, onSaveAsNew }: ViewChangesProps) {
  const [saving, setSaving] = useState(false)
  // read by the shortcut: a second Cmd/Ctrl+S before the re-render must still see the save in flight
  const savingRef = useRef(false)
  const [conflictDismissed, setConflictDismissed] = useState(false)
  const { dirty, canEdit } = controller

  const save = async (overwrite = false) => {
    if (savingRef.current) return
    savingRef.current = true
    setConflictDismissed(false)
    setSaving(true)
    try {
      // a 409 resolves and sets `controller.conflict`, which opens the conflict dialog with the edits kept
      await controller.save(overwrite ? { overwrite: true } : undefined)
    } catch (error) {
      toast.error(error instanceof ApiProblem ? error.detail : "Orbit couldn't update the view. Try again.")
    } finally {
      savingRef.current = false
      setSaving(false)
    }
  }

  // works even while typing; it only acts when there is something to save (and never animates)
  useCommand('view.save', () => {
    if (savingRef.current) return
    if (canEdit) void save()
    else onSaveAsNew({ instant: true })
  }, { enabled: dirty })
  const keyShortcuts = useShortcutLabel('view.save')

  return (
    <div role="group" aria-label="Unsaved view changes" className="flex shrink-0 animate-view-bar-enter items-center gap-1.5">
      <Button type="button" variant="ghost" size="sm" className="text-muted-foreground" title="Go back to the saved view" onClick={() => controller.discard()}>
        Reset
      </Button>
      {canEdit ? (
        <>
          <Button type="button" variant="ghost" size="sm" className="text-muted-foreground" onClick={() => onSaveAsNew()}>
            Save as new view
          </Button>
          <Button type="button" variant="outline" size="sm" disabled={saving} aria-keyshortcuts={keyShortcuts} onClick={() => void save()}>
            Update view
          </Button>
        </>
      ) : (
        <Button type="button" variant="outline" size="sm" aria-keyshortcuts={keyShortcuts} onClick={() => onSaveAsNew()}>
          <Bookmark className="size-3.5" />
          Save as new view
        </Button>
      )}
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
    </div>
  )
}
