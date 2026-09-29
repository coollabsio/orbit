// Generic destructive-confirm dialog (the chat reference ConfirmDelete modal) used across features.
import { useNavigate } from 'react-router'
import { Button } from '@/components/ui/button'
import { DialogFooter, DialogClose } from '@/components/ui/dialog'
import { Modal } from './Modal'

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
    <Modal title={title} onClose={onClose} className="sm:max-w-sm">
      <p className="text-sm leading-5 text-muted-foreground">{description}</p>
      <DialogFooter>
        <DialogClose render={<Button type="button" variant="outline" />}>
          Cancel
        </DialogClose>
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
      </DialogFooter>
    </Modal>
  )
}
