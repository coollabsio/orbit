import { Button } from '@/components/ui/button'
import { useChatContext } from '../../api/chatContext'
import type { Conversation } from '../../api/types'
import { fullTimestamp } from '../../lib/time'
import { useChatLocation, useChatNavigation } from '../../useChatNavigation'
import { conversationTitle, useChatPeople } from './people'

/** The block above the first message of a conversation. */
export function ConversationStart({ conversation }: { conversation: Conversation }) {
  const { currentUserId } = useChatContext()
  const people = useChatPeople()
  const location = useChatLocation()
  const { togglePane } = useChatNavigation()
  const title = conversationTitle(conversation, people, currentUserId)
  const membersOpen = location.view === 'conversation' && location.pane === 'members' && !location.thread && !location.q

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
        {creator ? `Created by ${creator.name} on ${fullTimestamp(conversation.createdAt)}` : `Created on ${fullTimestamp(conversation.createdAt)}`}
      </p>
      {conversation.isMember && !conversation.archived && !membersOpen ? (
        <Button variant="outline" size="sm" className="mt-2" onClick={() => togglePane('members')}>
          Add people
        </Button>
      ) : null}
    </div>
  )
}
