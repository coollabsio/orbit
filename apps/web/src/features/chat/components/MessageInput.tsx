// Port of the chat reference MessageInput: autosize textarea, @mention autocomplete with keyboard
// navigation, grouped emoji picker with search, + actions menu, reply bar.
import { useEffect, useImperativeHandle, useRef, useState, type ClipboardEvent, type KeyboardEvent, type Ref } from 'react'
import { Add, EmojiHappy, Magnifier, Paperclip2, Reply, Xmark } from 'reicon-react'
import { ThreadIcon } from '../../../components/ui/icons/ThreadIcon'
import { sendChatMessage } from '../../../mock/actions'
import { nextId } from '../../../mock/store'
import type { AppState, Attachment, Channel, ChatMessage } from '../../../mock/types'
import type { EmojiEntry } from '../emojis'
import { displayName, roleColor } from '../chatLib'
import { useMentionAutocomplete } from '../useMentionAutocomplete'
import { MentionPopover } from './MentionPopover'

const EMOJI_CATEGORY_ORDER = ['Smileys', 'Gestures', 'Symbols', 'Objects', 'Other']

function groupEmojisByCategory(emojis: EmojiEntry[]) {
  const groups = new Map<string, EmojiEntry[]>()
  emojis.forEach((entry) => {
    const category = emojiCategory(entry)
    groups.set(category, [...(groups.get(category) ?? []), entry])
  })
  return EMOJI_CATEGORY_ORDER.filter((category) => groups.has(category)).map((category) => ({
    category,
    items: groups.get(category)!,
  }))
}

function emojiCategory({ name, keywords }: EmojiEntry): string {
  const text = `${name} ${keywords}`
  if (/\b(face|smil|grin|laugh|tear|kiss|heart|angry|sad|sleep|sick|hot|cold|party|emotion)\b/.test(text)) {
    return 'Smileys'
  }
  if (/\b(hand|finger|fist|clap|thumb|wave|gesture|pray|writing)\b/.test(text)) {
    return 'Gestures'
  }
  if (/\b(button|symbol|arrow|sign|mark|circle|square|triangle|keycap|zodiac|cross|star)\b/.test(text)) {
    return 'Symbols'
  }
  if (
    /\b(tool|book|phone|computer|card|money|clock|mail|music|game|food|drink|sport|vehicle|building|house|medical|office|light|lock|key)\b/.test(
      text,
    )
  ) {
    return 'Objects'
  }
  return 'Other'
}

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
  const [emojiQuery, setEmojiQuery] = useState('')
  const [emojis, setEmojis] = useState<EmojiEntry[]>([])
  const [attachments, setAttachments] = useState<Attachment[]>([])
  const [threadMode, setThreadMode] = useState(false)
  const threadModeRef = useRef(false)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const composerRef = useRef<HTMLDivElement>(null)
  const mention = useMentionAutocomplete(state.users, text, setText, inputRef, resizeTextarea, (user) => roleColor(state, user.id))
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
    if (!emojiOpen || emojis.length > 0) return
    let cancelled = false
    import('../emojis').then(({ EMOJIS }) => {
      if (!cancelled) setEmojis(EMOJIS)
    })
    return () => {
      cancelled = true
    }
  }, [emojiOpen, emojis.length])

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
    const next = Array.from(files).map(
      (file): Attachment => ({
        id: nextId('att'),
        fileName: file.name,
        mimeType: file.type || 'application/octet-stream',
        fileSize: file.size,
        url: URL.createObjectURL(file),
      }),
    )
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

  /** the chat reference handlePaste: pasted images become attachments. */
  function handlePaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const pasted = [
      ...Array.from(event.clipboardData.files),
      ...Array.from(event.clipboardData.items)
        .filter((item) => item.kind === 'file' && item.type.startsWith('image/'))
        .map((item) => item.getAsFile())
        .filter((file): file is File => Boolean(file)),
    ].filter((file) => file.type.startsWith('image/'))
    const seen = new Set<string>()
    const files = pasted.filter((file) => {
      const key = `${file.type}:${file.size}`
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })
    if (files.length === 0) return
    event.preventDefault()
    addFiles(files)
  }

  function resizeTextarea() {
    const el = inputRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 320)}px`
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

  const normalizedEmojiQuery = emojiQuery.trim().toLowerCase()
  const filteredEmojis = normalizedEmojiQuery
    ? emojis.filter(
        ({ emoji, name, keywords }) =>
          emoji.includes(normalizedEmojiQuery) ||
          name.includes(normalizedEmojiQuery) ||
          keywords.includes(normalizedEmojiQuery),
      )
    : emojis
  const emojiGroups = groupEmojisByCategory(filteredEmojis)

  return (
    <div ref={composerRef} className="fc-composer">
      {attachments.length > 0 ? (
        <div className="fc-composer-attachments">
          <div className="fc-composer-attachments-grid">
            {attachments.map((a) => (
              <div key={a.id} className="fc-pending-attachment">
                {a.mimeType.startsWith('image/') ? <img src={a.url} alt="" /> : <Paperclip2 size={32} />}
                <span className="fc-pending-attachment-name">{a.fileName}</span>
                <button type="button" className="fc-pending-attachment-remove" title="Remove" onClick={() => removeAttachment(a.id)}>
                  <Xmark size={14} />
                </button>
              </div>
            ))}
          </div>
        </div>
      ) : null}
      {replyTarget ? (
        <div className="fc-composer-reply">
          <div className="fc-composer-reply-row">
            <Reply size={16} />
            <div style={{ minWidth: 0, flex: 1 }}>
              <div className="fc-composer-reply-title">Replying to {displayName(state, replyTarget)}</div>
              <div className="fc-composer-reply-preview">{replyTarget.content || 'No message content'}</div>
            </div>
            <button type="button" className="fc-composer-reply-close" title="Cancel reply" onClick={onCancelReply}>
              <Xmark />
            </button>
          </div>
        </div>
      ) : null}

      <div className="fc-input-row" data-attached={replyTarget || attachments.length > 0 ? 'true' : undefined}>
        <input ref={fileInputRef} type="file" multiple hidden aria-label="File upload" onChange={(e) => addFiles(e.target.files)} />
        {/* left plus menu */}
        <div style={{ position: 'relative', flexShrink: 0 }}>
          <button
            type="button"
            className="fc-input-button"
            data-active={actionsOpen ? 'true' : undefined}
            title="Add attachment or action"
            onClick={() => {
              setActionsOpen((prev) => !prev)
              setEmojiOpen(false)
            }}
          >
            <Add size={14} />
          </button>
          {actionsOpen ? (
            <div className="fc-composer-popover fc-actions-popover">
              <button type="button" onClick={() => fileInputRef.current?.click()}>
                <Paperclip2 size={16} />
                Upload Files
              </button>
              {showThreadAction ? (
                <button
                  type="button"
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
          <button type="button" className="fc-thread-pill" title="Cancel thread mode" onClick={() => setThreadModeImmediate(false)}>
            <ThreadIcon size={12} />
            Thread
          </button>
        ) : null}

        <textarea
          ref={inputRef}
          className="fc-input"
          value={text}
          rows={1}
          style={{ height: 24 }}
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
        <div style={{ position: 'relative', display: 'flex', flexShrink: 0, alignItems: 'center', gap: 2 }}>
          <button
            type="button"
            className="fc-input-button"
            data-active={emojiOpen ? 'true' : undefined}
            title="Emoji"
            onClick={() => {
              setEmojiOpen((prev) => !prev)
              setActionsOpen(false)
            }}
          >
            <EmojiHappy size={20} />
          </button>
          {emojiOpen ? (
            <div className="fc-composer-popover fc-emoji-popover">
              <div className="fc-popover-label" style={{ padding: '0 0 8px' }}>
                Emoji
              </div>
              <div className="fc-emoji-search">
                <Magnifier size={14} />
                <input
                  value={emojiQuery}
                  placeholder="Search emoji"
                  autoFocus
                  onChange={(e) => setEmojiQuery(e.target.value)}
                />
              </div>
              <div className="fc-emoji-scroll">
                {emojis.length === 0 ? (
                  <div className="fc-emoji-empty">Loading emoji...</div>
                ) : emojiGroups.length > 0 ? (
                  emojiGroups.map((group) => (
                    <section key={group.category} className="fc-emoji-group">
                      <div className="fc-emoji-group-label">{group.category}</div>
                      <div className="fc-emoji-grid">
                        {group.items.map(({ emoji, name }) => (
                          <button key={`${emoji}-${name}`} type="button" title={name} onClick={() => insertEmoji(emoji)}>
                            {emoji}
                          </button>
                        ))}
                      </div>
                    </section>
                  ))
                ) : (
                  <div className="fc-emoji-empty">No emoji found</div>
                )}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  )
}
