import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useChatContext } from '@/features/chat/api/chatContext'
import { usePresence } from '@/features/chat/api/liveStore'
import { useOpenDm } from '@/features/chat/api/mutations'
import { useChatNavigation } from '@/features/chat/useChatNavigation'
import { useMembers } from '@/features/workspaces/api'
import { chatErrorMessage, pickablePeople } from './channelLib'
import { MemberPicker } from './MemberPicker'

interface NewMessageDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/** Picks one or more people and opens the direct message with exactly them: the existing one, or a new one. */
export function NewMessageDialog({ open, onOpenChange }: NewMessageDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New message</DialogTitle>
          <DialogDescription>Choose one person, or several for a group conversation.</DialogDescription>
        </DialogHeader>
        <NewMessageForm onDone={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  )
}

function NewMessageForm({ onDone }: { onDone: () => void }) {
  const { workspaceId, currentUserId } = useChatContext()
  const members = useMembers(workspaceId).data ?? []
  const online = usePresence()
  const openDm = useOpenDm()
  const { openConversation } = useChatNavigation()
  const [selectedIds, setSelectedIds] = useState<string[]>([])

  const submit = async () => {
    if (selectedIds.length === 0 || openDm.isPending) return
    try {
      const conversation = await openDm.mutateAsync(selectedIds)
      onDone()
      openConversation(conversation.id)
    } catch {
      // The choice stays; the error shows above the buttons.
    }
  }

  return (
    <form
      className="grid gap-4"
      onSubmit={(event) => {
        event.preventDefault()
        void submit()
      }}
    >
      <MemberPicker
        autoFocus
        label="To"
        people={pickablePeople(members, currentUserId)}
        selectedIds={selectedIds}
        onChange={(ids) => {
          setSelectedIds(ids)
          if (openDm.isError) openDm.reset()
        }}
        online={online}
      />
      {openDm.isError ? (
        <p role="alert" className="text-sm text-destructive">
          {chatErrorMessage(openDm.error, 'The conversation was not opened. Try again.', { not_found: 'One of these people is no longer in the workspace.' })}
        </p>
      ) : null}
      <DialogFooter>
        <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
        <Button type="submit" disabled={selectedIds.length === 0 || openDm.isPending}>
          Open conversation
        </Button>
      </DialogFooter>
    </form>
  )
}
