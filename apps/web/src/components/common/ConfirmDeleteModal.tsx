// Generic destructive-confirm dialog (the chat reference ConfirmDelete modal) used across features.
import { useNavigate } from 'react-router'
import { Button } from '@/components/ui/button'
import { Modal } from './Modal'

const footerClass = 'mt-4 flex flex-wrap items-center justify-end gap-2 border-t border-border pt-4'

export function ConfirmDeleteModal({
  title,
  description,
  onClose,
  onConfirm,
  navigateAwayFrom,
  activeChannelId,
}: {
  title: string
  description: string
  onClose: () => void
  onConfirm: () => void
  navigateAwayFrom?: string | null
  activeChannelId?: string | null
}) {
  const navigate = useNavigate()
  return (
    <Modal title={title} onClose={onClose} maxWidth={384}>
      <p className="text-sm leading-5 text-muted-foreground">{description}</p>
      <div className={footerClass}>
        <Button type="button" variant="outline" onClick={onClose}>
          Cancel
        </Button>
        <Button
          type="button"
          variant="destructive"
          onClick={() => {
            onConfirm()
            if (navigateAwayFrom && navigateAwayFrom === activeChannelId) navigate('/chat')
          }}
        >
          Delete
        </Button>
      </div>
    </Modal>
  )
}
