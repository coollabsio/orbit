import { useState } from 'react'
import { Edit, MoreH } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { useChatContext } from '@/features/chat/api/chatContext'
import type { Conversation } from '@/features/chat/api/types'
import { conversationPath } from '@/features/chat/chatRoutes'
import { useHasDraft } from '@/features/chat/lib/drafts'
import { conversationBadge, conversationTitle } from '@/features/chat/lib/sidebar'
import type { User } from '@/features/workspaces/models'
import { ChatRow, ChatRowEnd, ChatRowLink, ChatRowName, CountBadge, UnreadMarker } from './ChatRow'
import { ConversationIcon } from './ConversationIcon'
import { ConversationMenuItems } from './ConversationMenuItems'
import { rowLabel } from './sidebarRows'
import { useConversationActions } from './useConversationActions'

/**
 * One conversation in the sidebar. Unread: bold name and the pink square, or a count in its place. Muted: muted text.
 * A draft shows a pencil in place of the leading icon. Right click opens the same menu as the `…` button.
 */
export function ConversationRow({
  conversation,
  people,
  active,
  onEdit,
}: {
  conversation: Conversation
  people: readonly User[]
  /** The open conversation, also while one of its threads is in full view. */
  active: boolean
  onEdit: (conversationId: string) => void
}) {
  const { workspaceId, currentUserId } = useChatContext()
  const actions = useConversationActions(conversation)
  const hasDraft = useHasDraft(workspaceId, conversation.id)
  const [menuOpen, setMenuOpen] = useState(false)
  const badge = conversationBadge(conversation, actions.state)
  const title = conversationTitle(conversation, people, currentUserId ?? '')
  // the open conversation shows its draft in the composer
  const draft = hasDraft && !active

  return (
    <ChatRow
      data-active={active}
      data-unread={badge.bold}
      data-muted={actions.state?.notify === 'muted'}
      onContextMenu={(event) => {
        event.preventDefault()
        setMenuOpen(true)
      }}
    >
      <ChatRowLink
        to={conversationPath(conversation.id)}
        aria-current={active ? 'page' : undefined}
        aria-label={rowLabel(title, badge, actions.state?.notify, draft)}
      >
        {draft ? (
          <span data-slot="conversation-icon" className="flex size-[18px] shrink-0 items-center justify-center text-muted-foreground">
            <Edit className="size-4" aria-hidden="true" />
          </span>
        ) : (
          <ConversationIcon conversation={conversation} people={people} currentUserId={currentUserId} size={conversation.kind === 'dm' ? 20 : 18} />
        )}
        <ChatRowName>{title}</ChatRowName>
      </ChatRowLink>
      <ChatRowEnd>
        <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
          <DropdownMenuTrigger
            render={
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={`Options for ${title}`}
                className="text-muted-foreground transition-none hover-fine:opacity-0 hover-fine:group-focus-within/row:opacity-100 hover-fine:group-hover/row:opacity-100 hover-fine:aria-expanded:opacity-100"
              />
            }
          >
            <MoreH className="size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent className="w-auto min-w-48">
            <ConversationMenuItems actions={actions} placement="row" onEdit={() => onEdit(conversation.id)} />
          </DropdownMenuContent>
        </DropdownMenu>
        {badge.count > 0 ? <CountBadge count={badge.count} /> : badge.marker ? <UnreadMarker /> : null}
      </ChatRowEnd>
    </ChatRow>
  )
}
