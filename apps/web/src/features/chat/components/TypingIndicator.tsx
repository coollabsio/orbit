// Port of the chat reference TypingIndicator (bouncing dots above the composer)
import { useEffect, useState } from 'react'
import type { AppState } from '../../../mock/types'

export function TypingIndicator({ state, channelId }: { state: AppState; channelId: string }) {
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    const interval = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(interval)
  }, [])

  const typing = (state.typingUsers[channelId] || []).filter(
    (t) => t.userId !== state.currentUserId && t.expires > now,
  )
  if (typing.length === 0) return null

  const names = typing.map(
    (t) => state.users.find((u) => u.id === t.userId)?.name.split(' ')[0] ?? 'Someone',
  )
  let text: string
  if (names.length === 1) {
    text = `${names[0]} is typing...`
  } else if (names.length === 2) {
    text = `${names[0]} and ${names[1]} are typing...`
  } else {
    text = `${names[0]} and ${names.length - 1} others are typing...`
  }

  return (
    <div className="absolute bottom-full left-4 pb-1">
      <div className="flex h-3 items-center gap-1.5 text-[11px] font-semibold text-muted-foreground">
        <div className="flex gap-0.5">
          <span className="inline-block size-1 rounded-full bg-muted-foreground motion-safe:animate-bounce [animation-delay:-0.3s]" />
          <span className="inline-block size-1 rounded-full bg-muted-foreground motion-safe:animate-bounce [animation-delay:-0.15s]" />
          <span className="inline-block size-1 rounded-full bg-muted-foreground motion-safe:animate-bounce" />
        </div>
        <span>{text}</span>
      </div>
    </div>
  )
}
