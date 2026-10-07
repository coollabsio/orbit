import { useState } from 'react'
import { toast } from 'sonner'
import { Crown, People, User as UserIcon, UserAdd } from 'reicon-react'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'
import { Tip } from '@/components/common/Tip'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useAddMembers, useOpenDm, useRemoveMember } from '@/features/chat/api/mutations'
import { useConversation } from '@/features/chat/api/queries'
import type { Conversation } from '@/features/chat/api/types'
import { canAddPeople, canManageChannel, chatErrorMessage, pickablePeople } from '@/features/chat/components/dialogs/channelLib'
import { memberGroups } from '@/features/chat/lib/memberGroups'
import { MemberPicker } from '@/features/chat/components/dialogs/MemberPicker'
import { useChatNavigation } from '@/features/chat/useChatNavigation'
import { CustomStatusText } from '@/features/realtime/components/CustomStatusText'
import { presenceOf, usePresence } from '@/features/realtime/presence'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { RightPane, RightPaneBody, RightPaneEmpty, RightPaneError, RightPaneLoading } from './RightPane'

/**
 * The people in a conversation, laid out like Discord's member list: online people under their role, offline people
 * dimmed at the end. A click on a person opens the direct message with them.
 */
export function MembersPane({ conversationId }: { conversationId: string }) {
  const { workspaceId, currentUserId } = useChatContext()
  const conversationQuery = useConversation(conversationId)
  const membersQuery = useMembers(workspaceId)
  const presence = usePresence()
  const openDm = useOpenDm()
  const removeMember = useRemoveMember()
  const { openConversation } = useChatNavigation()

  const conversation = conversationQuery.data
  const members = membersQuery.data ?? []
  const currentUser = members.find((member) => member.id === currentUserId)
  const people = (conversation?.memberIds ?? []).flatMap((id) => members.find((member) => member.id === id) ?? [])
  const groups = memberGroups(people, presence)
  // every workspace member stays in a public channel, and removing yourself is "Leave" in the conversation menu
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
      <RightPaneBody className="pt-0">
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
            <section key={group.label} aria-label={`${group.label}, ${group.people.length}`} data-online={group.online} className="group/members">
              <h3 className="px-2 pt-4 pb-1 text-xs font-semibold text-muted-foreground">
                {group.label} — {group.people.length}
              </h3>
              <ul className="flex flex-col gap-px">
                {group.people.map((person) => {
                  const shown = presenceOf(presence, person.id)
                  return (
                    <li
                      key={person.id}
                      className="group flex items-center gap-1 rounded-md transition-colors duration-150 ease-out focus-within:bg-muted hover-fine:hover:bg-muted group-data-[online=false]/members:opacity-40 group-data-[online=false]/members:focus-within:opacity-100 group-data-[online=false]/members:hover-fine:hover:opacity-100"
                    >
                      <Tip label={person.id === currentUserId ? 'Message yourself' : `Message ${person.name}`}>
                        <Button
                          variant="ghost"
                          className="h-[42px] min-w-0 flex-1 justify-start gap-3 px-2 text-[15px] font-medium text-muted-foreground group-focus-within:text-foreground hover:bg-transparent hover-fine:group-hover:text-foreground dark:hover:bg-transparent"
                          onClick={() => message(person)}
                        >
                          <UserAvatar user={person} size={32} status={shown.status} />
                          <span className="flex min-w-0 flex-col items-start leading-tight">
                            <span className="flex max-w-full min-w-0 items-center gap-1.5">
                              <span className="truncate">{person.name}</span>
                              {person.role === 'Owner' ? (
                                <>
                                  <Crown className="size-3.5 shrink-0 text-amber-500" aria-hidden="true" />
                                  <span className="sr-only">Workspace owner</span>
                                </>
                              ) : null}
                            </span>
                            {shown.emoji !== null || shown.text !== null ? (
                              <span className="max-w-full truncate text-xs font-normal text-muted-foreground">
                                <CustomStatusText emoji={shown.emoji} text={shown.text} />
                              </span>
                            ) : null}
                          </span>
                        </Button>
                      </Tip>
                      <Tip label="View profile">
                        <ProfileTrigger
                          userId={person.id}
                          name={person.name}
                          kind="plain"
                          render={
                            <Button
                              variant="ghost"
                              size="icon-sm"
                              aria-label="View profile"
                              className="mr-1 shrink-0 text-muted-foreground focus-visible:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100 hover-fine:group-focus-within:opacity-100"
                            />
                          }
                        >
                          <UserIcon aria-hidden="true" />
                        </ProfileTrigger>
                      </Tip>
                      {canRemove && person.id !== currentUserId ? (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="mr-1 text-muted-foreground focus-visible:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100 hover-fine:group-focus-within:opacity-100"
                          aria-label={`Remove ${person.name}`}
                          disabled={removeMember.isPending}
                          onClick={() => remove(person)}
                        >
                          Remove
                        </Button>
                      ) : null}
                    </li>
                  )
                })}
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
      <Tip label="Add people" side="bottom">
        <PopoverTrigger
          render={
            <Button variant="ghost" size="icon" aria-label="Add people">
              <UserAdd className="size-5" />
            </Button>
          }
        />
      </Tip>
      <PopoverContent align="end" className="w-80">
        <MemberPicker autoFocus label={`Add people to #${conversation.name}`} people={people} selectedIds={selectedIds} onChange={setSelectedIds} />
        <Button disabled={selectedIds.length === 0 || addMembers.isPending} onClick={submit}>
          {selectedIds.length > 1 ? `Add ${selectedIds.length} people` : 'Add people'}
        </Button>
      </PopoverContent>
    </Popover>
  )
}
