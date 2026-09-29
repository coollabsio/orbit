// the chat reference modals (Create/Edit Channel, Create/Edit Category) rendered
// with the app modal shell and shadcn form controls.
import { useId, useState } from 'react'
import { useNavigate } from 'react-router'
import { SmileCircle as Smile } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { DialogFooter, DialogClose } from '@/components/ui/dialog'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Emoji } from '@/components/common/Emoji'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import { Modal } from '@/components/common/Modal'
import {
  createChannel,
  createChatCategory,
  deleteChannel,
  deleteChatCategory,
  updateChannel,
  updateChatCategory,
} from '@/mock/actions'
import type { Channel, ChatCategory } from '@/mock/types'

export type ChannelModalState =
  | { kind: 'create-channel'; categoryId: string; categoryName: string }
  | { kind: 'create-category' }
  | { kind: 'edit-channel'; channel: Channel }
  | { kind: 'edit-category'; category: ChatCategory }
  | { kind: 'delete-channel'; channel: Channel }
  | { kind: 'delete-category'; id: string; name: string }
  | null

interface ChannelModalsProps {
  modal: ChannelModalState
  onClose: () => void
  activeChannelId: string | null
}

export function ChannelModals({ modal, onClose, activeChannelId }: ChannelModalsProps) {
  if (!modal) return null
  switch (modal.kind) {
    case 'create-channel':
      return <CreateChannelModal categoryId={modal.categoryId} categoryName={modal.categoryName} onClose={onClose} />
    case 'create-category':
      return <CreateCategoryModal onClose={onClose} />
    case 'edit-channel':
      return <EditChannelModal channel={modal.channel} onClose={onClose} />
    case 'edit-category':
      return <EditCategoryModal category={modal.category} onClose={onClose} />
    case 'delete-channel':
      return (
        <ConfirmDeleteModal
          title="Delete channel?"
          description={`This will permanently delete #${modal.channel.name} and its messages.`}
          onClose={onClose}
          onConfirm={() => {
            deleteChannel(modal.channel.id)
            onClose()
          }}
          navigateAwayFrom={modal.channel.id}
          activeChannelId={activeChannelId}
        />
      )
    case 'delete-category':
      return (
        <ConfirmDeleteModal
          title="Delete category?"
          description={`This will permanently delete ${modal.name} and all channels inside it.`}
          onClose={onClose}
          onConfirm={() => {
            deleteChatCategory(modal.id)
            onClose()
          }}
          navigateAwayFrom={null}
          activeChannelId={activeChannelId}
        />
      )
  }
}

function slugChannelName(name: string) {
  return name.trim().toLowerCase().replace(/\s+/g, '-')
}

/** Emoji tile that opens the picker panel (replaces the old text input + "Focus input"). */
function EmojiSelect({ value, onChange, label = 'Emoji' }: { value: string; onChange: (v: string) => void; label?: string }) {
  const [open, setOpen] = useState(false)
  const id = useId()
  return (
    <Field>
      <FieldLabel htmlFor={id}>
        {label} <span className="text-muted-foreground">(optional)</span>
      </FieldLabel>
      <div>
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger
            render={
              <Button
                id={id}
                type="button"
                variant="outline"
                size="icon"
                className="text-muted-foreground"
                aria-label={value.trim() ? `${label}: ${value}` : `Set ${label.toLowerCase()}`}
              />
            }
          >
            {value.trim() ? <Emoji value={value} size={20} /> : <Smile className="size-[18px]" />}
          </PopoverTrigger>
          <PopoverContent align="start" className="max-h-(--available-height) w-auto overflow-y-auto p-0">
            <EmojiPicker
              onPick={(emoji) => {
                onChange(emoji)
                setOpen(false)
              }}
              onRemove={
                value.trim()
                  ? () => {
                      onChange('')
                      setOpen(false)
                    }
                  : undefined
              }
            />
          </PopoverContent>
        </Popover>
      </div>
    </Field>
  )
}

function CreateChannelModal({ categoryId, categoryName, onClose }: { categoryId: string; categoryName: string; onClose: () => void }) {
  const [name, setName] = useState('')
  const [emoji, setEmoji] = useState('')
  const navigate = useNavigate()

  function commit(e: React.FormEvent) {
    e.preventDefault()
    const slug = slugChannelName(name)
    if (!slug) return
    const id = createChannel(categoryId, slug)
    if (emoji.trim()) updateChannel(id, { emoji: emoji.trim() })
    onClose()
    navigate(`/chat/${id}`)
  }

  return (
    <Modal title="Create Channel" description={`in ${categoryName}`} onClose={onClose} className="sm:max-w-md">
      <form className="flex flex-col gap-4" onSubmit={commit}>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="channel-name">
              Channel Name <span className="font-semibold text-primary">*</span>
            </FieldLabel>
            <Input id="channel-name" value={name} placeholder="general" autoFocus required onChange={(e) => setName(e.target.value)} />
          </Field>
          <EmojiSelect value={emoji} onChange={setEmoji} label="Channel emoji" />
        </FieldGroup>
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>
            Cancel
          </DialogClose>
          <Button type="submit" disabled={!name.trim()}>
            Create Channel
          </Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}

function CreateCategoryModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState('')
  const [emoji, setEmoji] = useState('')

  function commit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    createChatCategory(name.trim(), emoji.trim())
    onClose()
  }

  return (
    <Modal title="Create Category" description="Organize your channels into groups" onClose={onClose} className="sm:max-w-md">
      <form className="flex flex-col gap-4" onSubmit={commit}>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="category-name">
              Category Name <span className="font-semibold text-primary">*</span>
            </FieldLabel>
            <Input id="category-name" value={name} placeholder="New Category" autoFocus required onChange={(e) => setName(e.target.value)} />
          </Field>
          <EmojiSelect value={emoji} onChange={setEmoji} label="Category emoji" />
        </FieldGroup>
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>
            Cancel
          </DialogClose>
          <Button type="submit" disabled={!name.trim()}>
            Create Category
          </Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}

function EditChannelModal({ channel, onClose }: { channel: Channel; onClose: () => void }) {
  const [name, setName] = useState(channel.name)
  const [topic, setTopic] = useState(channel.description)
  const [emoji, setEmoji] = useState(channel.emoji ?? '')

  function commit(e: React.FormEvent) {
    e.preventDefault()
    const slug = slugChannelName(name)
    if (!slug) return
    updateChannel(channel.id, { name: slug, description: topic.trim(), emoji: emoji.trim() })
    onClose()
  }

  return (
    <Modal title="Edit Channel" onClose={onClose} className="sm:max-w-md">
      <form className="flex flex-col gap-4" onSubmit={commit}>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="edit-channel-name">
              Channel Name <span className="font-semibold text-primary">*</span>
            </FieldLabel>
            <Input id="edit-channel-name" value={name} placeholder="general" autoFocus required onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field>
            <FieldLabel htmlFor="edit-channel-topic">
              Topic <span className="text-muted-foreground">(optional)</span>
            </FieldLabel>
            <Input id="edit-channel-topic" value={topic} placeholder="What's this channel about?" onChange={(e) => setTopic(e.target.value)} />
          </Field>
          <EmojiSelect value={emoji} onChange={setEmoji} label="Channel emoji" />
        </FieldGroup>
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>
            Cancel
          </DialogClose>
          <Button type="submit" disabled={!name.trim()}>
            Save Changes
          </Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}

function EditCategoryModal({ category, onClose }: { category: ChatCategory; onClose: () => void }) {
  const [name, setName] = useState(category.name)
  const [emoji, setEmoji] = useState(category.emoji ?? '')

  function commit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    updateChatCategory(category.id, { name: name.trim(), emoji: emoji.trim() })
    onClose()
  }

  return (
    <Modal title="Edit Category" onClose={onClose} className="sm:max-w-md">
      <form className="flex flex-col gap-4" onSubmit={commit}>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="edit-category-name">
              Category Name <span className="font-semibold text-primary">*</span>
            </FieldLabel>
            <Input id="edit-category-name" value={name} placeholder="New Category" autoFocus required onChange={(e) => setName(e.target.value)} />
          </Field>
          <EmojiSelect value={emoji} onChange={setEmoji} label="Category emoji" />
        </FieldGroup>
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>
            Cancel
          </DialogClose>
          <Button type="submit" disabled={!name.trim()}>
            Save Changes
          </Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
