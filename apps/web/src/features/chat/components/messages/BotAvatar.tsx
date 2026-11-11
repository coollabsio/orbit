import { useContext, useEffect, useState } from 'react'
import { Cpu } from 'reicon-react'
import { Emoji } from '@/components/common/Emoji'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import type { MessageBot } from '@/features/chat/api/types'
import { CustomEmojiContext } from '@/lib/customEmojiContext'
import { emojiNamed, loadEmojiIndex } from '../../lib/emojiSearch'

/** The emoji of a name: a custom emoji of the workspace (`:name:`), or a standard one once the index has loaded. */
function useNamedEmoji(name: string | null | undefined): string | null {
  const custom = useContext(CustomEmojiContext)
  const [standard, setStandard] = useState<string | null>(null)
  useEffect(() => {
    if (!name || custom.has(name)) return
    void loadEmojiIndex().then((index) => setStandard(emojiNamed(index, name)), () => {})
  }, [name, custom])
  if (!name) return null
  return custom.has(name) ? `:${name}:` : standard
}

/** The avatar of an integration's message: its emoji or icon, or a chip on the muted surface. */
export function BotAvatar({ bot, size = 24 }: { bot: Pick<MessageBot, 'name' | 'iconUrl' | 'iconEmoji'>; size?: number }) {
  const emoji = useNamedEmoji(bot.iconEmoji)
  return (
    <Avatar data-slot="bot-avatar" className="after:border-transparent" style={{ width: size, height: size }} title={bot.name}>
      {bot.iconUrl && !emoji ? <AvatarImage src={bot.iconUrl} alt="" loading="eager" referrerPolicy="no-referrer" /> : null}
      <AvatarFallback className="bg-muted text-muted-foreground">
        {emoji ? (
          <Emoji value={emoji} className="size-[62%]" />
        ) : (
          <Cpu aria-hidden="true" style={{ width: Math.round(size * 0.55), height: Math.round(size * 0.55) }} />
        )}
      </AvatarFallback>
    </Avatar>
  )
}

/** "APP" after an integration's name, as Slack marks bots. */
export function BotBadge() {
  return (
    <span data-slot="bot-badge" className="shrink-0 rounded-sm bg-muted px-1 py-px text-[0.625rem] leading-4 font-semibold tracking-wide text-muted-foreground uppercase">
      App
    </span>
  )
}
