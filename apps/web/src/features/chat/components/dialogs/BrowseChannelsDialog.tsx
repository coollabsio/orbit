import { useState } from 'react'
import { toast } from 'sonner'
import { Hashtag } from 'reicon-react'
import { Button, buttonVariants } from '@/components/ui/button'
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useJoinChannel } from '@/features/chat/api/mutations'
import { useConversations } from '@/features/chat/api/queries'
import type { Conversation } from '@/features/chat/api/types'
import { useChatNavigation } from '@/features/chat/useChatNavigation'
import { chatErrorMessage } from './channelLib'
import { NewChannelDialog } from './NewChannelDialog'

interface BrowseChannelsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Every public channel of the workspace, with a search field. Choosing a channel joins it (when the user is not in it)
 * and opens it. "Create channel" closes this dialog and opens the new channel dialog with the searched text as its
 * name, so keep this component mounted while it is closed.
 */
export function BrowseChannelsDialog({ open, onOpenChange }: BrowseChannelsDialogProps) {
  const [newChannelName, setNewChannelName] = useState<string | null>(null)
  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Browse channels</DialogTitle>
            <DialogDescription>Public channels in this workspace.</DialogDescription>
          </DialogHeader>
          <ChannelList
            onDone={() => onOpenChange(false)}
            onCreate={(name) => {
              onOpenChange(false)
              setNewChannelName(name)
            }}
          />
        </DialogContent>
      </Dialog>
      <NewChannelDialog
        open={newChannelName !== null}
        defaultName={newChannelName ?? ''}
        onOpenChange={(next) => {
          if (!next) setNewChannelName(null)
        }}
      />
    </>
  )
}

function ChannelList({ onDone, onCreate }: { onDone: () => void; onCreate: (name: string) => void }) {
  const conversations = useConversations()
  const joinChannel = useJoinChannel()
  const { openConversation } = useChatNavigation()
  const [search, setSearch] = useState('')

  const query = search.trim().toLowerCase().replace(/^#/, '')
  const channels = (conversations.data ?? [])
    .filter((conversation) => conversation.kind === 'public' && !conversation.archived)
    .filter((channel) => !query || channel.name.includes(query) || channel.topic.toLowerCase().includes(query))
    .sort((a, b) => a.name.localeCompare(b.name))

  const choose = async (channel: Conversation) => {
    if (joinChannel.isPending) return
    if (!channel.isMember) {
      try {
        await joinChannel.mutateAsync(channel.id)
      } catch (error) {
        toast.error(chatErrorMessage(error, `Could not join #${channel.name}. Try again.`, { forbidden: `You cannot join #${channel.name}.`, not_found: `#${channel.name} no longer exists.` }))
        return
      }
    }
    onDone()
    openConversation(channel.id)
  }

  if (conversations.isError) {
    return (
      <div role="alert" className="flex min-h-40 flex-col items-center justify-center gap-2.5 text-sm">
        <p>The channels could not be loaded.</p>
        <Button variant="outline" size="sm" onClick={() => void conversations.refetch()}>
          Try again
        </Button>
      </div>
    )
  }

  return (
    <Command shouldFilter={false} label="Browse channels" className="h-auto gap-1 bg-transparent p-0">
      <CommandInput autoFocus placeholder="Search channels…" value={search} onValueChange={setSearch} />
      <CommandList className="h-80 max-h-[50dvh]">
        {conversations.isPending ? null : (
          <CommandEmpty className="flex flex-col items-center gap-2.5 py-10">
            <span>No channels match</span>
            <Button variant="outline" size="sm" onClick={() => onCreate(search)}>
              Create channel
            </Button>
          </CommandEmpty>
        )}
        {channels.map((channel) => (
          <CommandItem
            key={channel.id}
            value={channel.id}
            disabled={joinChannel.isPending}
            className="items-start gap-2.5 py-2"
            onSelect={() => void choose(channel)}
          >
            <Hashtag aria-hidden className="mt-0.5 text-muted-foreground" />
            <span className="flex min-w-0 flex-1 flex-col gap-0.5">
              <span className="truncate font-medium">{channel.name}</span>
              {channel.topic ? <span className="truncate text-xs text-muted-foreground">{channel.topic}</span> : null}
              <span className="text-xs text-muted-foreground">
                {channel.memberIds.length === 1 ? '1 member' : `${channel.memberIds.length} members`}
                {channel.isMember ? ' · Joined' : null}
              </span>
            </span>
            <span className={buttonVariants({ variant: 'outline', size: 'xs' })}>
              {channel.isMember ? 'Open' : 'Join'}
            </span>
          </CommandItem>
        ))}
      </CommandList>
    </Command>
  )
}
