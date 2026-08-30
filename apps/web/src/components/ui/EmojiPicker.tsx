import { useEffect, useMemo, useState } from 'react'
import { Magnifier } from 'reicon-react'
import type { EmojiEntry } from '../../features/chat/emojis'

/** Category buckets mirror the chat composer's emoji panel. */
const EMOJI_CATEGORY_ORDER = ['Smileys', 'Gestures', 'Symbols', 'Objects', 'Other']

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

/**
 * Standalone emoji picker panel (the reference app's icon picker shell): header with an optional
 * Remove action, search, grouped grid. The catalog is loaded lazily on first mount.
 */
export function EmojiPicker({ onPick, onRemove }: { onPick: (emoji: string) => void; onRemove?: () => void }) {
  const [emojis, setEmojis] = useState<EmojiEntry[]>([])
  const [query, setQuery] = useState('')

  useEffect(() => {
    let cancelled = false
    import('../../features/chat/emojis').then(({ EMOJIS }) => {
      if (!cancelled) setEmojis(EMOJIS)
    })
    return () => {
      cancelled = true
    }
  }, [])

  const groups = useMemo(() => {
    const q = query.trim().toLowerCase()
    const filtered = q
      ? emojis.filter(({ emoji, name, keywords }) => emoji.includes(q) || name.includes(q) || keywords.includes(q))
      : emojis
    const byCategory = new Map<string, EmojiEntry[]>()
    filtered.forEach((entry) => {
      const category = emojiCategory(entry)
      byCategory.set(category, [...(byCategory.get(category) ?? []), entry])
    })
    return EMOJI_CATEGORY_ORDER.filter((category) => byCategory.has(category)).map((category) => ({
      category,
      items: byCategory.get(category)!,
    }))
  }, [emojis, query])

  return (
    <div className="emoji-picker">
      <div className="emoji-picker-head">
        <span className="fc-popover-label">Emoji</span>
        {onRemove ? (
          <button type="button" className="button button-ghost emoji-picker-remove" onClick={onRemove}>
            Remove
          </button>
        ) : null}
      </div>
      <div className="fc-emoji-search">
        <Magnifier size={14} />
        <input value={query} placeholder="Search emoji" autoFocus aria-label="Search emoji" onChange={(e) => setQuery(e.target.value)} />
      </div>
      <div className="fc-emoji-scroll">
        {emojis.length === 0 ? (
          <div className="fc-emoji-empty">Loading emoji...</div>
        ) : groups.length > 0 ? (
          groups.map((group) => (
            <section key={group.category} className="fc-emoji-group">
              <div className="fc-emoji-group-label">{group.category}</div>
              <div className="fc-emoji-grid">
                {group.items.map(({ emoji, name }) => (
                  <button key={`${emoji}-${name}`} type="button" title={name} onClick={() => onPick(emoji)}>
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
  )
}
