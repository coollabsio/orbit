import { useEffect, useState } from 'react'
import { useChatContext } from '../../api/chatContext'
import type { Message } from '../../api/types'

/**
 * The message that a composer replies to. It is not part of the draft: it lasts until the reply is sent or cancelled,
 * the view closes, or the message is deleted.
 */
export function useReplyTarget() {
  const { client } = useChatContext()
  const [replyTo, setReplyTo] = useState<Message | null>(null)
  const targetId = replyTo?.id

  useEffect(() => {
    if (!client || !targetId) return
    return client.subscribe((event) => {
      // A root with replies is not removed: it comes as an update with `deleted`.
      const gone =
        event.type === 'message.deleted' ? event.messageId === targetId : event.type === 'message.updated' && event.message.id === targetId && event.message.deleted
      if (gone) setReplyTo(null)
    })
  }, [client, targetId])

  return [replyTo, setReplyTo] as const
}
