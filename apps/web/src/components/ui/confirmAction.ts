export interface ConfirmationOptions {
  title: string
  description?: string
  confirmLabel?: string
  cancelLabel?: string
  danger?: boolean
}

type ConfirmationHandler = (options: ConfirmationOptions) => Promise<boolean>
let requestConfirmation: ConfirmationHandler | undefined

/** Also callable from mutation error handlers. Without a mounted host, cancel safely. */
export function confirmAction(options: ConfirmationOptions): Promise<boolean> {
  return requestConfirmation?.(options) ?? Promise.resolve(false)
}

export function registerConfirmationHandler(handler: ConfirmationHandler) {
  requestConfirmation = handler
  return () => {
    if (requestConfirmation === handler) requestConfirmation = undefined
  }
}
