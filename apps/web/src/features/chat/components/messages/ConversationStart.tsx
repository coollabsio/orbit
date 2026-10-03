import { Button } from '@/components/ui/button'
import { useChatContext } from '../../api/chatContext'
import type { Conversation } from '../../api/types'
import { isSelfDm } from '../../lib/sidebar'
import { fullTimestamp } from '../../lib/time'
import { useChatNavigation, useOpenPane } from '../../useChatNavigation'
import { conversationTitle, useChatPeople } from './people'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'

/** The block above the first message of a conversation. */
export function ConversationStart({ conversation }: { conversation: Conversation }) {
  const { currentUserId } = useChatContext()
  const people = useChatPeople()
  const { togglePane } = useChatNavigation()
  const title = conversationTitle(conversation, people, currentUserId)
  const membersOpen = useOpenPane() === 'members'

  if (isSelfDm(conversation)) {
    return (
      <div data-slot="conversation-start" className="flex flex-col items-start gap-1 px-4 pt-8 pb-4 max-[899px]:px-3">
        <p className="text-base font-semibold">This is your space</p>
        <p className="max-w-[90ch] text-sm text-muted-foreground">Draft messages, keep links and files handy, or jot down notes. Only you can see it.</p>
      </div>
    )
  }

  if (conversation.kind === 'dm') {
    return (
      <div data-slot="conversation-start" className="px-4 pt-8 pb-4 max-[899px]:px-3">
        <p className="text-base font-semibold">This is the start of your conversation with {title}</p>
      </div>
    )
  }

  const creator = people.byId.get(conversation.createdBy)
  return (
    <div data-slot="conversation-start" className="flex flex-col items-start gap-1 px-4 pt-8 pb-4 max-[899px]:px-3">
      <p className="text-base font-semibold">This is the start of {title}</p>
      {conversation.topic ? <p className="max-w-[90ch] text-sm text-muted-foreground">{conversation.topic}</p> : null}
      <p className="text-xs text-muted-foreground">
        {creator ? (
          <>
            Created by{' '}
            <ProfileTrigger userId={creator.id} name={creator.name} className="font-medium text-foreground/85">
              {creator.name}
            </ProfileTrigger>{' '}
            on {fullTimestamp(conversation.createdAt)}
          </>
        ) : (
          `Created on ${fullTimestamp(conversation.createdAt)}`
        )}
      </p>
      {conversation.isMember && !conversation.archived && !membersOpen ? (
        <Button variant="outline" size="sm" className="mt-2" onClick={() => togglePane('members')}>
          Add people
        </Button>
      ) : null}
    </div>
  )
}
