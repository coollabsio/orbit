import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
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
  const cancelRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    const pendingRequests = queue.current
    const unregister = registerConfirmationHandler((options) => new Promise<boolean>((resolve) => {
      const pending = { ...options, id: nextId.current++, resolve }
      pendingRequests.push(pending)
      if (pendingRequests.length === 1) setRequest(pending)
    }))
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

  useEffect(() => {
    cancelRef.current?.focus()
  }, [request])

  if (!request) return null

  return createPortal(
    <Modal key={request.id} title={request.title} description={request.description} onClose={cancel} maxWidth={480}>
      <div className="confirmation-actions">
        <button ref={cancelRef} type="button" className="button" onClick={cancel}>
          {request.cancelLabel ?? 'Cancel'}
        </button>
        <button type="button" className={`button ${request.danger ? 'button-danger' : 'button-primary'}`} onClick={() => settle(true)}>
          {request.confirmLabel ?? 'Confirm'}
        </button>
      </div>
    </Modal>,
    document.body,
  )
}
