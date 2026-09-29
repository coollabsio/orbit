import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { registerConfirmationHandler, type ConfirmationOptions } from './confirmAction'

interface ConfirmationRequest extends ConfirmationOptions {
  id: number
  resolve: (confirmed: boolean) => void
}

/** Mount once at the app root. Queue requests so simultaneous errors don't lose a choice.
    A choice resolves at once; the dialog keeps showing that request while it plays its exit,
    then the next queued request opens in a fresh dialog (focus starts on Cancel again). */
export function ConfirmationModalHost() {
  const queue = useRef<ConfirmationRequest[]>([])
  const nextId = useRef(0)
  const [request, setRequest] = useState<ConfirmationRequest>()
  const [open, setOpen] = useState(false)
  // True from a request opening until its exit has finished; requests arriving meanwhile only queue.
  const showing = useRef(false)

  useEffect(() => {
    const pendingRequests = queue.current
    const unregister = registerConfirmationHandler(
      (options) =>
        new Promise<boolean>((resolve) => {
          const pending = { ...options, id: nextId.current++, resolve }
          pendingRequests.push(pending)
          if (!showing.current) {
            showing.current = true
            setRequest(pending)
            setOpen(true)
          }
        }),
    )
    return () => {
      unregister()
      pendingRequests.splice(0).forEach((pending) => pending.resolve(false))
    }
  }, [])

  // Ignore a second click while the dialog plays its exit: it would answer the next queued request.
  const settle = (confirmed: boolean) => {
    if (!open) return
    queue.current.shift()?.resolve(confirmed)
    setOpen(false)
  }
  const cancel = () => settle(false)

  // After the exit: show the next queued request, or unmount.
  const exitComplete = () => {
    const next = queue.current[0]
    showing.current = next !== undefined
    setRequest(next)
    setOpen(next !== undefined)
  }

  if (!request) return null

  return (
    <Dialog
      key={request.id}
      open={open}
      onOpenChange={(next) => {
        if (!next) cancel()
      }}
      onOpenChangeComplete={(next) => {
        if (!next) exitComplete()
      }}
    >
      <DialogContent className="sm:max-w-120">
        <DialogHeader>
          <DialogTitle>{request.title}</DialogTitle>
          {request.description ? <DialogDescription>{request.description}</DialogDescription> : null}
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={cancel}>
            {request.cancelLabel ?? 'Cancel'}
          </Button>
          <Button variant={request.danger ? 'destructive' : 'default'} onClick={() => settle(true)}>
            {request.confirmLabel ?? 'Confirm'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
