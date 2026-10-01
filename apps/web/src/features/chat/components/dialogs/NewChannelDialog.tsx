import { useId, useState } from 'react'
import { Hashtag, Lock } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { useChatContext } from '@/features/chat/api/chatContext'
import { usePresence } from '@/features/chat/api/liveStore'
import { useCreateChannel } from '@/features/chat/api/mutations'
import { useCategories } from '@/features/chat/api/queries'
import { ChatError } from '@/features/chat/api/types'
import { useChatNavigation } from '@/features/chat/useChatNavigation'
import { useMembers } from '@/features/workspaces/api'
import { CHANNEL_NAME_MAX, channelNameFinal, channelNameInput, chatErrorMessage, pickablePeople } from './channelLib'
import { MemberPicker } from './MemberPicker'

const NO_CATEGORY = 'none'

interface NewChannelDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The category the dialog was opened from. */
  defaultCategoryId?: string | null
  /** A name to start with (Browse channels passes the text that matched nothing). */
  defaultName?: string
}

/** Creates a public or private channel, then opens it. The form is mounted only while the dialog is open, so it starts empty each time. */
export function NewChannelDialog({ open, onOpenChange, defaultCategoryId, defaultName }: NewChannelDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New channel</DialogTitle>
          <DialogDescription>Channels keep a conversation about one topic in one place.</DialogDescription>
        </DialogHeader>
        <NewChannelForm defaultCategoryId={defaultCategoryId ?? null} defaultName={defaultName ?? ''} onDone={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  )
}

function NewChannelForm({ defaultCategoryId, defaultName, onDone }: { defaultCategoryId: string | null; defaultName: string; onDone: () => void }) {
  const { workspaceId, currentUserId } = useChatContext()
  const categories = useCategories().data ?? []
  const members = useMembers(workspaceId).data ?? []
  const online = usePresence()
  const createChannel = useCreateChannel()
  const { openConversation } = useChatNavigation()
  const [name, setName] = useState(() => channelNameInput(defaultName))
  const [topic, setTopic] = useState('')
  const [category, setCategory] = useState(defaultCategoryId ?? NO_CATEGORY)
  const [kind, setKind] = useState<'public' | 'private'>('public')
  const [memberIds, setMemberIds] = useState<string[]>([])
  const nameId = useId()
  const topicId = useId()
  const categoryId = useId()

  const finalName = channelNameFinal(name)
  const nameTaken = createChannel.error instanceof ChatError && createChannel.error.code === 'conflict'
  const categoryOptions = [{ value: NO_CATEGORY, label: 'No category' }, ...categories.map((item) => ({ value: item.id, label: item.name }))]
  // the categories can still load when the dialog opens, and the default one can be gone
  const categoryValue = categoryOptions.some((option) => option.value === category) ? category : NO_CATEGORY

  const submit = async () => {
    if (!finalName || createChannel.isPending) return
    try {
      const channel = await createChannel.mutateAsync({
        name: finalName,
        topic: topic.trim(),
        categoryId: categoryValue === NO_CATEGORY ? null : categoryValue,
        kind,
        memberIds: kind === 'private' ? memberIds : [],
      })
      onDone()
      openConversation(channel.id)
    } catch {
      // The form stays; the error shows under the name or above the buttons.
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
      <Field data-invalid={nameTaken || undefined}>
        <FieldLabel htmlFor={nameId}>Name</FieldLabel>
        <InputGroup>
          <InputGroupAddon>
            <Hashtag aria-hidden />
          </InputGroupAddon>
          <InputGroupInput
            id={nameId}
            autoFocus
            required
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            maxLength={CHANNEL_NAME_MAX}
            placeholder="design-team"
            value={name}
            aria-invalid={nameTaken || undefined}
            onChange={(event) => {
              setName(channelNameInput(event.target.value))
              if (createChannel.isError) createChannel.reset()
            }}
          />
        </InputGroup>
        {nameTaken ? <FieldError>A channel with this name already exists.</FieldError> : <FieldDescription>Lowercase, with hyphens in place of spaces.</FieldDescription>}
      </Field>

      <Field>
        <FieldLabel htmlFor={topicId}>Topic</FieldLabel>
        <Input id={topicId} value={topic} maxLength={250} placeholder="What this channel is about" onChange={(event) => setTopic(event.target.value)} />
      </Field>

      <Field>
        <FieldLabel htmlFor={categoryId}>Category</FieldLabel>
        <Select items={categoryOptions} value={categoryValue} onValueChange={(value) => setCategory(value as string)}>
          <SelectTrigger id={categoryId}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {categoryOptions.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>

      <FieldSet className="gap-0">
        <FieldLegend variant="label">Visibility</FieldLegend>
        <ToggleGroup
          variant="outline"
          orientation="vertical"
          className="w-full"
          value={[kind]}
          onValueChange={(value) => {
            // a press on the chosen one must not leave the channel without a kind
            const next = value.at(-1)
            if (next === 'public' || next === 'private') setKind(next)
          }}
        >
          <ToggleGroupItem value="public" className="h-auto flex-col items-start gap-0.5 py-2 whitespace-normal">
            <span className="flex items-center gap-1.5">
              <Hashtag aria-hidden />
              Public
            </span>
            <span className="text-xs font-normal text-muted-foreground">Anyone in the workspace can find it and join.</span>
          </ToggleGroupItem>
          <ToggleGroupItem value="private" className="h-auto flex-col items-start gap-0.5 py-2 whitespace-normal">
            <span className="flex items-center gap-1.5">
              <Lock aria-hidden />
              Private
            </span>
            <span className="text-xs font-normal text-muted-foreground">Only the people you add can see it.</span>
          </ToggleGroupItem>
        </ToggleGroup>
      </FieldSet>

      {kind === 'private' ? (
        <Field>
          <FieldLabel>Add people</FieldLabel>
          <MemberPicker label="Add people" people={pickablePeople(members, currentUserId)} selectedIds={memberIds} onChange={setMemberIds} online={online} />
          <FieldDescription>You are a member already. You can add more people later.</FieldDescription>
        </Field>
      ) : null}

      {createChannel.isError && !nameTaken ? (
        <p role="alert" className="text-sm text-destructive">
          {chatErrorMessage(createChannel.error, 'The channel was not created. Try again.', { not_found: 'This category no longer exists. Choose another one.' })}
        </p>
      ) : null}

      <DialogFooter>
        <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
        <Button type="submit" disabled={!finalName || createChannel.isPending}>
          {createChannel.isPending ? 'Creating…' : 'Create channel'}
        </Button>
      </DialogFooter>
    </form>
  )
}
