import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import { Textarea } from '@/components/ui/textarea'
import { MESSAGE_MAX_LENGTH, type Message } from '../../api/types'
import { decodeMentions, encodeMentions } from '../../lib/mentionTokens'
import { useAutosize } from '../composer/useAutosize'
import { isCoarsePointer } from './environment'
import { useMessageList } from './messageListContext'

/** Edits a message in place. `Enter` saves, `Esc` cancels (and goes no further: it does not close the pane). */
export function MessageEditor({ message }: { message: Message }) {
  const { people, saveEdit, endEdit } = useMessageList()
  const [text, setText] = useState(() => decodeMentions(message.body, people.members, people.channels))
  const input = useRef<HTMLTextAreaElement>(null)
  useAutosize(input, text)

  useEffect(() => {
    const element = input.current
    if (!element) return
    element.focus()
    element.setSelectionRange(element.value.length, element.value.length)
  }, [])

  const body = encodeMentions(text.trim(), people.members, people.channels)
  const tooLong = body.length > MESSAGE_MAX_LENGTH
  const canSave = !tooLong && (body !== '' || message.attachments.length > 0)

  function save() {
    if (!canSave) return
    if (body === message.body) endEdit()
    else saveEdit(message, body)
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      endEdit()
      return
    }
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing && !isCoarsePointer()) {
      event.preventDefault()
      save()
    }
  }

  return (
    <div data-slot="message-editor" data-realtime-safe="" className="flex flex-col gap-1.5 py-1">
      <Textarea
        ref={input}
        value={text}
        rows={1}
        aria-label="Edit message"
        aria-invalid={tooLong || undefined}
        className="max-h-[40cqh] min-h-9 resize-none bg-background"
        onChange={(event) => setText(event.target.value)}
        onKeyDown={onKeyDown}
      />
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Button size="sm" disabled={!canSave} onClick={save}>
          Save
        </Button>
        <Button size="sm" variant="ghost" onClick={endEdit}>
          Cancel
        </Button>
        {tooLong ? (
          <span className="text-destructive">{body.length - MESSAGE_MAX_LENGTH} characters over the limit</span>
        ) : (
          <span className="max-[899px]:hidden">
            <Kbd>Enter</Kbd> to save, <Kbd>Esc</Kbd> to cancel
          </span>
        )}
      </div>
    </div>
  )
}
