import { useEffect, useId, useRef } from 'react'
import { X } from 'reicon-react'

interface ModalProps {
  title: string
  description?: string
  onClose: () => void
  children: React.ReactNode
  /** Coolify modal-input: lg:min-w-2xl (672px). */
  maxWidth?: number
}

/** Coolify `x-modal-input`: overlay + settings-section shell (header strip, nested body panel). */
export function Modal({ title, description, onClose, children, maxWidth = 672 }: ModalProps) {
  const titleId = useId()
  const descriptionId = useId()
  const modalRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      if (e.key !== 'Tab') return
      const focusable = modalRef.current?.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
      )
      if (!focusable?.length) return
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault()
        last.focus()
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault()
        first.focus()
      }
    }
    const previouslyFocused = document.activeElement as HTMLElement | null
    document.addEventListener('keydown', onKeyDown)
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    const frame = requestAnimationFrame(() => {
      if (!modalRef.current?.contains(document.activeElement)) {
        modalRef.current?.querySelector<HTMLElement>('input, textarea, select')?.focus()
      }
    })
    return () => {
      cancelAnimationFrame(frame)
      document.removeEventListener('keydown', onKeyDown)
      document.body.style.overflow = previousOverflow
      previouslyFocused?.focus()
    }
  }, [onClose])

  return (
    <div
      className="modal-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="modal"
        ref={modalRef}
        style={{ maxWidth }}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descriptionId : undefined}
      >
        <header className="modal-header">
          <div className="modal-heading">
            <h3 id={titleId} className="modal-title">
              {title}
            </h3>
            {description ? (
              <p id={descriptionId} className="modal-description">
                {description}
              </p>
            ) : null}
          </div>
          <button type="button" className="icon-button" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
        </header>
        <div className="modal-body">{children}</div>
      </div>
    </div>
  )
}
