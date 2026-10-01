import { useState } from 'react'
import { toast } from 'sonner'
import { People, UserAdd } from 'reicon-react'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useChatContext } from '@/features/chat/api/chatContext'
import { usePresence } from '@/features/chat/api/liveStore'
import { useAddMembers, useOpenDm, useRemoveMember } from '@/features/chat/api/mutations'
import { useConversation } from '@/features/chat/api/queries'
import type { Conversation } from '@/features/chat/api/types'
import { canAddPeople, canManageChannel, chatErrorMessage, pickablePeople } from '@/features/chat/components/dialogs/channelLib'
import { MemberPicker } from '@/features/chat/components/dialogs/MemberPicker'
import { useChatNavigation } from '@/features/chat/useChatNavigation'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { RightPane, RightPaneBody, RightPaneEmpty, RightPaneError, RightPaneLoading } from './RightPane'

/** The people in a conversation, grouped Online / Offline. A click on a person opens the direct message with them. */
export function MembersPane({ conversationId }: { conversationId: string }) {
  const { workspaceId, currentUserId } = useChatContext()
  const conversationQuery = useConversation(conversationId)
  const membersQuery = useMembers(workspaceId)
  const online = usePresence()
  const openDm = useOpenDm()
  const removeMember = useRemoveMember()
  const { openConversation } = useChatNavigation()

  const conversation = conversationQuery.data
  const members = membersQuery.data ?? []
  const currentUser = members.find((member) => member.id === currentUserId)
  const people = (conversation?.memberIds ?? [])
    .flatMap((id) => members.find((member) => member.id === id) ?? [])
    .sort((a, b) => a.name.localeCompare(b.name))
  const groups = [
    { label: 'Online', people: people.filter((person) => online.has(person.id)) },
    { label: 'Offline', people: people.filter((person) => !online.has(person.id)) },
  ].filter((group) => group.people.length > 0)
  // in #general everybody stays, and removing yourself is "Leave" in the conversation menu
  const canRemove = Boolean(conversation && conversation.kind === 'private' && !conversation.isDefault && canManageChannel(conversation, currentUser))

  const message = (person: User) => {
    if (openDm.isPending) return
    openDm.mutate(person.id === currentUserId ? [] : [person.id], {
      onSuccess: (dm) => openConversation(dm.id),
      onError: (error) => toast.error(chatErrorMessage(error, `Could not open the conversation with ${person.name}. Try again.`)),
    })
  }

  const remove = (person: User) => {
    removeMember.mutate(
      { conversationId, userId: person.id },
      {
        onError: (error) =>
          toast.error(chatErrorMessage(error, `Could not remove ${person.name}. Try again.`, { forbidden: 'Only the channel creator and workspace owners and admins can remove people.' })),
      },
    )
  }

  return (
    <RightPane
      title="Members"
      actions={conversation && canAddPeople(conversation, currentUser) ? <AddPeople conversation={conversation} people={pickablePeople(members, currentUserId, conversation.memberIds)} /> : null}
    >
      <RightPaneBody>
        {conversationQuery.isPending || membersQuery.isPending ? (
          <RightPaneLoading />
        ) : conversationQuery.isError || membersQuery.isError ? (
          <RightPaneError
            title="The members could not be loaded."
            onRetry={() => {
              void conversationQuery.refetch()
              void membersQuery.refetch()
            }}
          />
        ) : people.length === 0 ? (
          <RightPaneEmpty icon={People} title="No members to show" description="Open a conversation from the sidebar to see who is in it." />
        ) : (
          groups.map((group) => (
            <section key={group.label} aria-label={`${group.label}, ${group.people.length}`} className="mb-2">
              <h3 className="px-2 py-1 text-xs text-muted-foreground">
                {group.label} · {group.people.length}
              </h3>
              <ul className="flex flex-col">
                {group.people.map((person) => (
                  <li key={person.id} className="group flex items-center gap-1">
                    <Button
                      variant="ghost"
                      className="min-w-0 flex-1 justify-start gap-2 px-2 font-normal"
                      title={person.id === currentUserId ? 'Message yourself' : `Message ${person.name}`}
                      onClick={() => message(person)}
                    >
                      <UserAvatar user={person} size={24} showOnline online={online.has(person.id)} />
                      <span className="truncate">{person.name}</span>
                      {person.id === currentUserId ? <span className="text-xs text-muted-foreground">you</span> : null}
                      {person.role !== 'Member' ? <span className="text-xs text-muted-foreground">{person.role}</span> : null}
                    </Button>
                    {canRemove && person.id !== currentUserId ? (
                      <Button
                        variant="ghost"
                        size="sm"
                        className="text-muted-foreground focus-visible:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100 hover-fine:group-focus-within:opacity-100"
                        aria-label={`Remove ${person.name}`}
                        disabled={removeMember.isPending}
                        onClick={() => remove(person)}
                      >
                        Remove
                      </Button>
                    ) : null}
                  </li>
                ))}
              </ul>
            </section>
          ))
        )}
      </RightPaneBody>
    </RightPane>
  )
}

/** "Add people": a popover with the picker. The people on offer are workspace members who are not in the channel yet. */
function AddPeople({ conversation, people }: { conversation: Conversation; people: User[] }) {
  const online = usePresence()
  const addMembers = useAddMembers()
  const [open, setOpen] = useState(false)
  const [selectedIds, setSelectedIds] = useState<string[]>([])

  const submit = () => {
    if (selectedIds.length === 0 || addMembers.isPending) return
    addMembers.mutate(
      { conversationId: conversation.id, userIds: selectedIds },
      {
        onSuccess: () => {
          setOpen(false)
          setSelectedIds([])
        },
        onError: (error) =>
          toast.error(
            chatErrorMessage(error, 'Could not add people. Try again.', {
              forbidden: 'Only the channel creator and workspace owners and admins can add people to a private channel.',
              not_found: 'One of these people is no longer in the workspace.',
            }),
          ),
      },
    )
  }

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) setSelectedIds([])
      }}
    >
      <PopoverTrigger
        render={
          <Button variant="ghost" size="icon" aria-label="Add people" title="Add people">
            <UserAdd weight="Filled" className="size-5" />
          </Button>
        }
      />
      <PopoverContent align="end" className="w-80">
        <MemberPicker autoFocus label={`Add people to #${conversation.name}`} people={people} selectedIds={selectedIds} onChange={setSelectedIds} online={online} />
        <Button disabled={selectedIds.length === 0 || addMembers.isPending} onClick={submit}>
          {selectedIds.length > 1 ? `Add ${selectedIds.length} people` : 'Add people'}
        </Button>
      </PopoverContent>
    </Popover>
  )
}
