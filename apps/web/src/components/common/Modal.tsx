import { useState, type ReactNode } from 'react'
import { cn } from 'cn'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'

interface ModalProps {
  title: string
  description?: string
  onClose: () => void
  children: ReactNode
  className?: string
  /** When false, Escape, backdrop and × do nothing (e.g. while a request the dialog started is pending). */
  dismissible?: boolean
}

/** App modal built on the shadcn Dialog. Callers mount it while open; a dismiss (Escape, backdrop, ×)
    plays the exit first and calls `onClose` once it has finished. Base UI handles focus trap, Escape,
    scroll lock and backdrop dismiss. */
export function Modal({ title, description, onClose, children, className, dismissible = true }: ModalProps) {
  const [open, setOpen] = useState(true)
  return (
    <Dialog
      open={open}
      onOpenChange={(next, details) => {
        if (next) return
        if (dismissible) setOpen(false)
        else details.cancel()
      }}
      onOpenChangeComplete={(next) => {
        if (!next) onClose()
      }}
    >
      <DialogContent className={cn('sm:max-w-2xl', className)}>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description ? <DialogDescription>{description}</DialogDescription> : null}
        </DialogHeader>
        {children}
      </DialogContent>
    </Dialog>
  )
}
