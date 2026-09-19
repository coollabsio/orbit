// Port of the chat reference MessageInput: autosize textarea, @mention autocomplete with keyboard
// navigation, grouped emoji picker with search, + actions menu, reply bar.
import { useEffect, useImperativeHandle, useRef, useState, type ClipboardEvent, type KeyboardEvent, type Ref } from 'react'
import { Paperclip, Plus, Reply, SmilePlus, X } from 'lucide-react'
import { EmojiPicker } from '../../../components/ui/EmojiPicker'
import { ThreadIcon } from '../../../components/ui/icons/ThreadIcon'
import { sendChatMessage } from '../../../mock/actions'
import type { AppState, Attachment, Channel, ChatMessage } from '../../../mock/types'
import { clipboardFiles, fileToAttachment } from '../attachmentLib'
import { displayName, roleColor } from '../chatLib'
import { useMentionAutocomplete } from '../useMentionAutocomplete'
import { MentionPopover } from './MentionPopover'

const inputButtonClass =
  'flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground/90 transition-colors hover:bg-muted hover:text-foreground data-[active=true]:text-primary'
const actionsButtonClass =
  'flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm font-semibold text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-45 [&>svg]:size-4 [&>svg]:text-muted-foreground'

export interface MessageInputHandle {
  addFiles: (files: FileList | File[]) => void
  focus: () => void
}

export function MessageInput({
  ref,
  state,
  channel,
  replyTarget = null,
  onCancelReply,
  threadRootId = null,
  placeholder,
  onSend,
  showThreadAction = true,
  threadActionLabel = 'Create Thread',
  onCreateThread,
  autoFocus,
}: {
  /** the chat reference: ChatArea drops files into the composer through this handle. */
  ref?: Ref<MessageInputHandle>
  state: AppState
  /** Channel to send into; omit when `onSend` handles the message (task comments). */
  channel?: Channel
  replyTarget?: ChatMessage | null
  onCancelReply?: () => void
  /** the chat reference ThreadPanel: send replies into this thread instead of the channel timeline. */
  threadRootId?: string | null
  placeholder?: string
  /** Override sending (the chat reference NewThreadPanel, task comments). Return false to keep the draft. */
  onSend?: (content: string, attachments: Attachment[]) => boolean | void
  /** Show the thread action in the + menu (off inside thread panels). */
  showThreadAction?: boolean
  threadActionLabel?: string
  /** When given, the + menu action opens the new-thread panel; otherwise it enters thread mode (pill). */
  onCreateThread?: () => void
  autoFocus?: boolean
}) {
  const [text, setText] = useState('')
  const [actionsOpen, setActionsOpen] = useState(false)
  const [emojiOpen, setEmojiOpen] = useState(false)
  const [attachments, setAttachments] = useState<Attachment[]>([])
  const [threadMode, setThreadMode] = useState(false)
  const threadModeRef = useRef(false)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const composerRef = useRef<HTMLDivElement>(null)
  const mention = useMentionAutocomplete(state.users, text, setText, inputRef, resizeTextarea, (user) => roleColor(state, user.id), state.channels)
  const closeMention = mention.close

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (composerRef.current && !composerRef.current.contains(e.target as Node)) {
        setActionsOpen(false)
        setEmojiOpen(false)
        closeMention()
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [closeMention])

useEffect(() => {
    if (!replyTarget && !autoFocus) return
    requestAnimationFrame(() => inputRef.current?.focus())
  }, [replyTarget, autoFocus])

  function setThreadModeImmediate(enabled: boolean) {
    threadModeRef.current = enabled
    setThreadMode(enabled)
    if (enabled) requestAnimationFrame(() => inputRef.current?.focus())
  }

  /** Mock upload: files become attachments immediately (object URLs stand in for uploaded files). */
  function addFiles(files: FileList | File[] | null) {
    if (!files || files.length === 0) return
    const next = Array.from(files).map(fileToAttachment)
    setAttachments((prev) => [...prev, ...next])
    setActionsOpen(false)
    if (fileInputRef.current) fileInputRef.current.value = ''
    inputRef.current?.focus()
  }

  useImperativeHandle(ref, () => ({ addFiles, focus: () => inputRef.current?.focus() }))

  function removeAttachment(id: string) {
    setAttachments((prev) => {
      const attachment = prev.find((a) => a.id === id)
      if (attachment?.url.startsWith('blob:')) URL.revokeObjectURL(attachment.url)
      return prev.filter((a) => a.id !== id)
    })
  }

  /** Pasted files (screenshots, copied images, files) become attachments; plain text pastes as usual. */
  function handlePaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const files = clipboardFiles(event)
    if (files.length === 0) return
    event.preventDefault()
    addFiles(files)
  }

  function resizeTextarea() {
    const el = inputRef.current
    if (!el) return
    el.style.height = 'auto'
    const height = Math.min(el.scrollHeight, 320)
    el.style.height = `${height}px`
    el.style.overflowY = el.scrollHeight > 320 ? 'auto' : 'hidden'
  }

  function handleChange(value: string, cursor: number) {
    setText(value)
    mention.update(value, cursor)
    requestAnimationFrame(resizeTextarea)
  }

  function insertEmoji(emoji: string) {
    const el = inputRef.current
    const start = el?.selectionStart ?? text.length
    const end = el?.selectionEnd ?? text.length
    const next = `${text.slice(0, start)}${emoji}${text.slice(end)}`
    setText(next)
    setEmojiOpen(false)
    requestAnimationFrame(() => {
      if (!el) return
      el.focus()
      const pos = start + emoji.length
      el.selectionStart = pos
      el.selectionEnd = pos
      resizeTextarea()
    })
  }

  function handleSend() {
    const trimmed = text.trim()
    if (!trimmed && attachments.length === 0) return
    const startsThread = threadModeRef.current
    if (onSend) {
      if (onSend(trimmed, attachments) === false) return
    } else if (channel) {
      sendChatMessage(channel.id, trimmed, startsThread ? null : (replyTarget?.id ?? null), {
        threadRootId,
        attachments,
        startsThread,
      })
    }
    setText('')
    setAttachments([])
    setThreadModeImmediate(false)
    onCancelReply?.()
    mention.close()
    setTimeout(() => {
      if (inputRef.current) inputRef.current.style.height = '24px'
      inputRef.current?.focus()
    }, 0)
  }

  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (mention.handleKeyDown(e)) return
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  return (
    <div ref={composerRef} className="relative shrink-0 px-3 pb-2">
      {attachments.length > 0 ? (
        <div className="rounded-t-lg border border-b-0 border-border bg-muted/25 p-2">
          <div className="grid grid-cols-2 gap-1 min-[900px]:grid-cols-4">
            {attachments.map((a) => (
              <div key={a.id} className="flex items-center gap-2 rounded-lg bg-background p-2 transition-colors hover:bg-muted">
                {a.mimeType.startsWith('image/') ? <img src={a.url} alt="" className="size-14 shrink-0 rounded-md border border-border object-cover" /> : <Paperclip className="size-8 shrink-0 text-muted-foreground" />}
                <span className="min-w-0 flex-1 truncate text-sm font-medium text-foreground">{a.fileName}</span>
                <button type="button" className="inline-flex size-7 shrink-0 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-muted" title="Remove" onClick={() => removeAttachment(a.id)}>
                  <X className="size-3.5" />
                </button>
              </div>
            ))}
          </div>
        </div>
      ) : null}
      {replyTarget ? (
        <div className="rounded-t-lg border border-b-0 border-border bg-muted/25 px-3 py-2">
          <div className="flex min-w-0 items-center gap-2">
            <Reply className="size-4 shrink-0 text-primary" />
            <div className="min-w-0 flex-1">
              <div className="text-xs leading-5 font-bold text-foreground">Replying to {displayName(state, replyTarget)}</div>
              <div className="truncate text-xs leading-5 font-semibold text-muted-foreground">{replyTarget.content || 'No message content'}</div>
            </div>
            <button type="button" className="grid size-7 shrink-0 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground" title="Cancel reply" onClick={onCancelReply}>
              <X className="size-3.5" />
            </button>
          </div>
        </div>
      ) : null}

      <div
        className="relative flex min-h-11 items-start gap-2 rounded-lg border border-border bg-muted/25 px-2 py-1.5 transition-[border-color,box-shadow] focus-within:border-primary/60 focus-within:ring-2 focus-within:ring-primary/15 data-[attached=true]:rounded-t-none"
        data-attached={replyTarget || attachments.length > 0 ? 'true' : undefined}
      >
        <input ref={fileInputRef} type="file" multiple hidden aria-label="File upload" onChange={(e) => addFiles(e.target.files)} />
        {/* left plus menu */}
        <div className="relative shrink-0">
          <button
            type="button"
            className={inputButtonClass}
            data-active={actionsOpen ? 'true' : undefined}
            title="Add attachment or action"
            onClick={() => {
              setActionsOpen((prev) => !prev)
              setEmojiOpen(false)
            }}
          >
            <Plus className="size-3.5" />
          </button>
          {actionsOpen ? (
            <div className="absolute bottom-full left-0 z-50 mb-4 w-56 rounded-lg border border-border bg-popover p-1.5 shadow-xl">
              <button type="button" className={actionsButtonClass} onClick={() => fileInputRef.current?.click()}>
                <Paperclip size={16} />
                Upload Files
              </button>
              {showThreadAction ? (
                <button
                  type="button"
                  className={actionsButtonClass}
                  onClick={() => {
                    if (onCreateThread) onCreateThread()
                    else setThreadModeImmediate(true)
                    setActionsOpen(false)
                    inputRef.current?.focus()
                  }}
                >
                  <ThreadIcon size={16} />
                  {threadActionLabel}
                </button>
              ) : null}
            </div>
          ) : null}
        </div>

        {threadMode ? (
          <button type="button" className="mt-1 inline-flex h-6 shrink-0 items-center gap-1 rounded-md bg-primary/10 px-2 text-xs font-semibold text-primary transition-colors hover:bg-primary/20" title="Cancel thread mode" onClick={() => setThreadModeImmediate(false)}>
            <ThreadIcon size={12} />
            Thread
          </button>
        ) : null}

        <textarea
          ref={inputRef}
          className="max-h-80 min-h-6 min-w-0 flex-1 resize-none border-none bg-transparent py-1 text-sm leading-6 font-semibold text-foreground outline-none placeholder:font-semibold placeholder:text-muted-foreground/45 max-[899px]:text-[13px]"
          value={text}
          rows={1}
          style={{ height: 24, overflowY: 'hidden' }}
          placeholder={placeholder ?? (channel ? `${threadMode ? 'Start a thread' : 'Message'} #${channel.name}` : 'Message')}
          onChange={(e) => handleChange(e.target.value, e.target.selectionStart)}
          onClick={(e) => mention.update(text, e.currentTarget.selectionStart)}
          onSelect={(e) => mention.update(text, e.currentTarget.selectionStart)}
          onKeyDown={handleKeyDown}
          onPaste={handlePaste}
          onInput={resizeTextarea}
        />

        {mention.open ? (
          <MentionPopover
            suggestions={mention.suggestions}
            activeIndex={mention.activeIndex}
            onSelect={mention.insert}
            onHover={mention.setActiveIndex}
          />
        ) : null}

        {/* right emoji menu */}
        <div className="relative flex shrink-0 items-center gap-0.5">
          <button
            type="button"
            className={inputButtonClass}
            data-active={emojiOpen ? 'true' : undefined}
            title="Emoji"
            onClick={() => {
              setEmojiOpen((prev) => !prev)
              setActionsOpen(false)
            }}
          >
            <SmilePlus className="size-5" />
          </button>
          {emojiOpen ? (
            <div className="absolute right-0 bottom-full z-50 mb-4 rounded-xl border border-border bg-popover shadow-xl">
              <EmojiPicker onPick={insertEmoji} />
            </div>
          ) : null}
        </div>
      </div>
    </div>
  )
}
