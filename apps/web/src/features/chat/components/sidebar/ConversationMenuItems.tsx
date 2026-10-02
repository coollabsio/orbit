import { Archive, Edit, Link2, Logout, Notification, Star, TickCircle } from 'reicon-react'
import {
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from '@/components/ui/dropdown-menu'
import type { NotifyLevel } from '@/features/chat/api/types'
import type { ConversationActions } from './useConversationActions'

/**
 * The items of a conversation's `…` menu, inside a `DropdownMenuContent`. The sidebar row (`placement="row"`) also has
 * "Mark as read"; the header does not.
 */
export function ConversationMenuItems({ actions, placement, onEdit }: { actions: ConversationActions; placement: 'row' | 'header'; onEdit: () => void }) {
  const { state } = actions
  const row = placement === 'row'
  return (
    <>
      {state ? (
        <>
          {row ? (
            <DropdownMenuItem disabled={state.unreadCount === 0 && state.mentionCount === 0} onClick={actions.markRead}>
              <TickCircle />
              Mark as read
            </DropdownMenuItem>
          ) : null}
          <DropdownMenuItem onClick={actions.toggleFavorite}>
            <Star weight={state.favorite ? 'Filled' : 'Outline'} />
            {state.favorite ? 'Remove star' : 'Star'}
          </DropdownMenuItem>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              <Notification />
              Notifications
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-auto min-w-40">
              <DropdownMenuRadioGroup value={state.notify} onValueChange={(value) => actions.setNotify(value as NotifyLevel)}>
                <DropdownMenuRadioItem value="all">All messages</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="mentions">Mentions only</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="muted">Muted</DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        </>
      ) : null}
      <DropdownMenuItem onClick={actions.copyLink}>
        <Link2 />
        Copy link
      </DropdownMenuItem>
      {actions.canManage ? (
        <>
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={onEdit}>
            <Edit />
            Edit channel
          </DropdownMenuItem>
        </>
      ) : null}
      {actions.canLeave || actions.canArchive ? <DropdownMenuSeparator /> : null}
      {actions.canLeave ? (
        <DropdownMenuItem onClick={() => void actions.leave()}>
          <Logout />
          Leave channel
        </DropdownMenuItem>
      ) : null}
      {actions.canArchive ? (
        <DropdownMenuItem variant="destructive" onClick={() => void actions.archive()}>
          <Archive />
          Archive channel
        </DropdownMenuItem>
      ) : null}
    </>
  )
}
