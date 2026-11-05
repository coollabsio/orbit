import { Button } from '@/components/ui/button'
import { useDismissEphemeral } from '../../api/mutations'
import { useEphemeralMessages } from '../../api/queries'
import { BotAvatar, BotBadge } from './BotAvatar'
import { MessageBody } from './MessageBody'

/**
 * An integration's replies that only the user sees (Slack's ephemeral messages), above the composer. They are not
 * stored: a reload or Dismiss takes them away. Their buttons do nothing, as the message does not exist on the server.
 */
export function EphemeralMessages({ conversationId }: { conversationId: string }) {
  const messages = useEphemeralMessages(conversationId)
  const dismiss = useDismissEphemeral()
  if (messages.length === 0) return null
  return (
    <div data-slot="ephemeral-messages" className="grid max-h-[40vh] gap-1 overflow-y-auto px-4 pb-2 max-[899px]:px-3">
      {messages.map((message) => (
        <div key={message.id} data-slot="ephemeral-message" className="flex gap-3 rounded-lg bg-muted/50 px-3 py-2 text-[15px] max-[899px]:text-sm">
          {message.bot ? <BotAvatar bot={message.bot} size={32} /> : null}
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="truncate font-medium text-foreground">{message.bot?.name}</span>
              <BotBadge />
              <span className="shrink-0 text-xs text-muted-foreground">Only visible to you</span>
              <Button type="button" variant="ghost" size="xs" className="ml-auto text-muted-foreground" onClick={() => dismiss(message)}>
                Dismiss
              </Button>
            </div>
            <MessageBody message={{ ...message, bot: message.bot && { ...message.bot, interactive: false } }} />
          </div>
        </div>
      ))}
    </div>
  )
}
