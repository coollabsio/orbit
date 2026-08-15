import { useEffect } from 'react'
import { CloseCircle } from 'reicon-react'

interface ModalProps {
  title: string
  onClose: () => void
  children: React.ReactNode
  maxWidth?: number
}

export function Modal({ title, onClose, children, maxWidth = 480 }: ModalProps) {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  return (
    <div
      className="modal-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="modal" style={{ maxWidth }} role="dialog" aria-label={title}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            minHeight: 48,
            padding: '8px 8px 8px 16px',
            flexShrink: 0,
          }}
        >
          <span style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-muted)' }}>{title}</span>
          <button className="icon-button" onClick={onClose} aria-label="Close">
            <CloseCircle size={16} />
          </button>
        </div>
        <div style={{ padding: 16, paddingTop: 0, overflowY: 'auto' }}>{children}</div>
      </div>
    </div>
  )
}
