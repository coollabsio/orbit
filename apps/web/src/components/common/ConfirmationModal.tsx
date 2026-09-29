import { useCallback, useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { DialogFooter } from '@/components/ui/dialog'
import { Modal } from './Modal'
import { registerConfirmationHandler, type ConfirmationOptions } from './confirmAction'

interface ConfirmationRequest extends ConfirmationOptions {
  id: number
  resolve: (confirmed: boolean) => void
}

/** Mount once at the app root. Queue requests so simultaneous errors don't lose a choice. */
export function ConfirmationModalHost() {
  const queue = useRef<ConfirmationRequest[]>([])
  const nextId = useRef(0)
  const [request, setRequest] = useState<ConfirmationRequest>()

  useEffect(() => {
    const pendingRequests = queue.current
    const unregister = registerConfirmationHandler(
      (options) =>
        new Promise<boolean>((resolve) => {
          const pending = { ...options, id: nextId.current++, resolve }
          pendingRequests.push(pending)
          if (pendingRequests.length === 1) setRequest(pending)
        }),
    )
    return () => {
      unregister()
      pendingRequests.splice(0).forEach((pending) => pending.resolve(false))
    }
  }, [])

  const settle = useCallback((confirmed: boolean) => {
    queue.current.shift()?.resolve(confirmed)
    setRequest(queue.current[0])
  }, [])
  const cancel = useCallback(() => settle(false), [settle])

  if (!request) return null

  return (
    <Modal key={request.id} title={request.title} description={request.description} onClose={cancel} className="sm:max-w-120">
      <DialogFooter>
        <Button variant="outline" onClick={cancel}>
          {request.cancelLabel ?? 'Cancel'}
        </Button>
        <Button variant={request.danger ? 'destructive' : 'default'} onClick={() => settle(true)}>
          {request.confirmLabel ?? 'Confirm'}
        </Button>
      </DialogFooter>
    </Modal>
  )
}
