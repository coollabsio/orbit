import type { ReactNode } from 'react'
import { cn } from 'cn'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'

interface ModalProps {
  title: string
  description?: string
  onClose: () => void
  children: ReactNode
  className?: string
}

/** App modal built on the shadcn Dialog. Mounted only while open; Base UI handles
    focus trap, Escape, scroll lock and backdrop dismiss. */
export function Modal({ title, description, onClose, children, className }: ModalProps) {
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose()
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
