import { useState } from 'react'
import { Copy, Edit, Link2, MoreH, Trash } from 'reicon-react'
import { toast } from 'sonner'
import { cn } from 'cn'
import { ApiProblem } from '@/api/problem'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { useDeleteView, type SavedView } from '../api/views'
import { confirmDeleteView, copyViewLink } from '../viewActions'

export interface ViewActionsMenuProps {
  workspaceId: string
  view: Pick<SavedView, 'id' | 'name' | 'visibility' | 'can_edit'>
  onEdit: () => void
  onDuplicate: () => void
  /** Called once the view is gone (after the confirm and a successful DELETE). */
  onDeleted?: () => void
  /** Put the view's name in the trigger label, for lists with one menu per row. */
  named?: boolean
  align?: 'start' | 'end'
  className?: string
}

/** Edit, Duplicate, Copy link, and Delete view for a saved view (view header, Views page rows). */
export function ViewActionsMenu({ workspaceId, view, onEdit, onDuplicate, onDeleted, named = false, align = 'start', className }: ViewActionsMenuProps) {
  const deleteView = useDeleteView(workspaceId)
  const [open, setOpen] = useState(false)

  const remove = async () => {
    if (!(await confirmDeleteView(view))) return
    try {
      await deleteView.mutateAsync(view.id)
      onDeleted?.()
    } catch (error) {
      toast.error(error instanceof ApiProblem ? error.detail : "Orbit couldn't delete the view.")
    }
  }

  return (
    <DropdownMenu open={open} onOpenChange={setOpen} modal={false}>
      <DropdownMenuTrigger
        render={
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={named ? `${view.name} options` : 'View options'}
            className={cn('text-muted-foreground', className)}
          >
            <MoreH />
          </Button>
        }
      />
      <DropdownMenuContent align={align} className="w-auto min-w-44">
        {view.can_edit ? <DropdownMenuItem onClick={onEdit}><Edit />Edit view</DropdownMenuItem> : null}
        <DropdownMenuItem onClick={onDuplicate}><Copy />Duplicate</DropdownMenuItem>
        <DropdownMenuItem onClick={() => void copyViewLink(workspaceId, view.id)}><Link2 />Copy link</DropdownMenuItem>
        {view.can_edit ? (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="destructive" onClick={() => void remove()}><Trash />Delete view</DropdownMenuItem>
          </>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
