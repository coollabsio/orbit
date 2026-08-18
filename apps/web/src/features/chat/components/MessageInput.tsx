// Port of the chat reference MessageInput: autosize textarea, @mention autocomplete with keyboard
// navigation, grouped emoji picker with search, + actions menu, reply bar.
import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { Add, EmojiHappy, Magnifier, Paperclip2, Reply, User, Xmark } from 'reicon-react'
import { sendChatMessage } from '../../../mock/actions'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import type { EmojiEntry } from '../emojis'
import { displayName } from '../chatLib'

interface MentionState {
  start: number
  end: number
  query: string
}

interface MentionSuggestion {
  id: string
  label: string
  username: string
  color: string
}

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

export function MessageInput({
  state,
  channel,
  replyTarget,
  onCancelReply,
}: {
  state: AppState
  channel: Channel
  replyTarget: ChatMessage | null
  onCancelReply: () => void
}) {
  const [text, setText] = useState('')
  const [actionsOpen, setActionsOpen] = useState(false)
  const [emojiOpen, setEmojiOpen] = useState(false)
  const [emojiQuery, setEmojiQuery] = useState('')
  const [emojis, setEmojis] = useState<EmojiEntry[]>([])
  const [mentionState, setMentionState] = useState<MentionState | null>(null)
  const [activeMentionIndex, setActiveMentionIndex] = useState(0)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const composerRef = useRef<HTMLDivElement>(null)
  const suppressMentionRef = useRef(false)

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (composerRef.current && !composerRef.current.contains(e.target as Node)) {
        setActionsOpen(false)
        setEmojiOpen(false)
        setMentionState(null)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

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
    if (!replyTarget) return
    requestAnimationFrame(() => inputRef.current?.focus())
  }, [replyTarget])

  function resizeTextarea() {
    const el = inputRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 320)}px`
  }

  const mentionSuggestions: MentionSuggestion[] = state.users.map((user) => ({
    id: user.id,
    label: user.name,
    username: user.handle,
    color: user.color,
  }))
  const normalizedMentionQuery = mentionState?.query.trim().toLowerCase() ?? ''
  const filteredMentionSuggestions = (
    normalizedMentionQuery
      ? mentionSuggestions.filter(
          (s) =>
            s.label.toLowerCase().includes(normalizedMentionQuery) ||
            s.username.toLowerCase().includes(normalizedMentionQuery),
        )
      : mentionSuggestions
  ).slice(0, 10)

  function updateMentionState(value: string, cursor: number) {
    if (suppressMentionRef.current) {
      suppressMentionRef.current = false
      setMentionState(null)
      return
    }
    const beforeCursor = value.slice(0, cursor)
    const atIndex = beforeCursor.lastIndexOf('@')
    if (atIndex === -1) {
      setMentionState(null)
      return
    }
    const query = beforeCursor.slice(atIndex + 1)
    const charBeforeAt = atIndex > 0 ? beforeCursor[atIndex - 1] : ''
    if ((charBeforeAt && !/\s/.test(charBeforeAt)) || query.includes('\n') || query.length > 48) {
      setMentionState(null)
      return
    }
    setMentionState({ start: atIndex, end: cursor, query })
    setActiveMentionIndex(0)
  }

  function handleChange(value: string, cursor: number) {
    setText(value)
    updateMentionState(value, cursor)
    requestAnimationFrame(resizeTextarea)
  }

  function insertMention(suggestion: MentionSuggestion) {
    if (!mentionState) return
    const el = inputRef.current
    const insertion = `@${suggestion.username} `
    const next = `${text.slice(0, mentionState.start)}${insertion}${text.slice(mentionState.end)}`
    const cursor = mentionState.start + insertion.length
    suppressMentionRef.current = true
    setText(next)
    setMentionState(null)
    requestAnimationFrame(() => {
      if (!el) return
      el.focus()
      el.selectionStart = cursor
      el.selectionEnd = cursor
      resizeTextarea()
    })
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
    if (!trimmed) return
    sendChatMessage(channel.id, trimmed, replyTarget?.id ?? null)
    setText('')
    onCancelReply()
    setMentionState(null)
    setTimeout(() => {
      if (inputRef.current) inputRef.current.style.height = '24px'
      inputRef.current?.focus()
    }, 0)
  }

  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (mentionState && filteredMentionSuggestions.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setActiveMentionIndex((index) => (index + 1) % filteredMentionSuggestions.length)
        return
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault()
        setActiveMentionIndex(
          (index) => (index - 1 + filteredMentionSuggestions.length) % filteredMentionSuggestions.length,
        )
        return
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault()
        insertMention(filteredMentionSuggestions[activeMentionIndex])
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        setMentionState(null)
        return
      }
    }
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

      <div className="fc-input-row" data-attached={replyTarget ? 'true' : undefined}>
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
              <button type="button" disabled title="File uploads arrive with the backend">
                <Paperclip2 size={16} />
                Upload Files
              </button>
            </div>
          ) : null}
        </div>

        <textarea
          ref={inputRef}
          className="fc-input"
          value={text}
          rows={1}
          style={{ height: 24 }}
          placeholder={`Message #${channel.name}`}
          onChange={(e) => handleChange(e.target.value, e.target.selectionStart)}
          onClick={(e) => updateMentionState(text, e.currentTarget.selectionStart)}
          onSelect={(e) => updateMentionState(text, e.currentTarget.selectionStart)}
          onKeyDown={handleKeyDown}
          onInput={resizeTextarea}
        />

        {mentionState && filteredMentionSuggestions.length > 0 ? (
          <div className="fc-mention-popover">
            <div className="fc-popover-label">Mentions</div>
            {filteredMentionSuggestions.map((suggestion, index) => (
              <button
                key={suggestion.id}
                type="button"
                className="fc-mention-option"
                data-active={index === activeMentionIndex ? 'true' : undefined}
                onMouseDown={(event) => {
                  event.preventDefault()
                  insertMention(suggestion)
                }}
                onMouseEnter={() => setActiveMentionIndex(index)}
              >
                <span
                  className="fc-mention-avatar"
                  style={{ background: `color-mix(in srgb, ${suggestion.color} 22%, transparent)`, color: suggestion.color }}
                >
                  {suggestion.label.charAt(0).toUpperCase()}
                </span>
                <span className="fc-mention-label" style={{ color: suggestion.color }}>
                  @{suggestion.label}
                </span>
                <span className="fc-mention-username">
                  <User size={12} />
                  {suggestion.username}
                </span>
              </button>
            ))}
          </div>
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
