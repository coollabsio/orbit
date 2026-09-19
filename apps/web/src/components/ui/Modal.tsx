import type { ReactNode } from 'react'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './dialog'

interface ModalProps {
  title: string
  description?: string
  onClose: () => void
  children: ReactNode
  maxWidth?: number
}

/** App modal built on the shadcn Dialog. Mounted only while open; Base UI handles
    focus trap, Escape, scroll lock and backdrop dismiss. */
export function Modal({ title, description, onClose, children, maxWidth = 672 }: ModalProps) {
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <DialogContent style={{ maxWidth }}>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description ? <DialogDescription>{description}</DialogDescription> : null}
        </DialogHeader>
        {children}
      </DialogContent>
    </Dialog>
  )
}
