import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'

export interface ConflictDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onReload: () => void
  onOverwrite: () => void
  pending?: boolean
}

export function ConflictDialog({ open, onOpenChange, onReload, onOverwrite, pending = false }: ConflictDialogProps) {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>This view changed since you opened it</AlertDialogTitle>
          <AlertDialogDescription>Reload to see the latest version and drop your changes, or overwrite it with your changes.</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <Button type="button" variant="outline" disabled={pending} onClick={onReload}>Reload</Button>
          <Button type="button" variant="destructive" disabled={pending} onClick={onOverwrite}>Overwrite</Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
