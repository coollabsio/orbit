import { Cpu } from 'reicon-react'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import type { MessageBot } from '@/features/chat/api/types'

/** The avatar of an integration's message: its icon, or a chip on the muted surface. */
export function BotAvatar({ bot, size = 24 }: { bot: Pick<MessageBot, 'name' | 'iconUrl'>; size?: number }) {
  return (
    <Avatar data-slot="bot-avatar" className="after:border-transparent" style={{ width: size, height: size }} title={bot.name}>
      {bot.iconUrl ? <AvatarImage src={bot.iconUrl} alt="" loading="eager" referrerPolicy="no-referrer" /> : null}
      <AvatarFallback className="bg-muted text-muted-foreground">
        <Cpu aria-hidden="true" style={{ width: Math.round(size * 0.55), height: Math.round(size * 0.55) }} />
      </AvatarFallback>
    </Avatar>
  )
}

/** "APP" after an integration's name, as Slack marks bots. */
export function BotBadge() {
  return (
    <span data-slot="bot-badge" className="shrink-0 rounded-sm bg-muted px-1 py-px text-[10px] leading-4 font-semibold tracking-wide text-muted-foreground uppercase">
      App
    </span>
  )
}
