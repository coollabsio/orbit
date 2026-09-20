import { useEffect, useMemo, useState } from 'react'
import { SearchNormal as Search } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { useAppState } from '@/mock/store'
import type { EmojiEntry } from '@/lib/emojis'

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

const groupLabelClass =
  'sticky top-0 z-10 mb-1 border-b border-border bg-popover/95 py-1 text-[10px] font-bold tracking-wider text-muted-foreground uppercase'
const gridClass = 'grid grid-cols-8 gap-1'
const gridButtonClass = 'size-8 rounded-lg border-0 text-lg hover:bg-muted dark:hover:bg-muted'

/**
 * Standalone emoji picker panel (the reference app's icon picker shell): header with an optional
 * Remove action, search, grouped grid. The catalog is loaded lazily on first mount.
 */
export function EmojiPicker({ onPick, onRemove }: { onPick: (emoji: string) => void; onRemove?: () => void }) {
  const { customEmojis } = useAppState()
  const [emojis, setEmojis] = useState<EmojiEntry[]>([])
  const [query, setQuery] = useState('')

  useEffect(() => {
    let cancelled = false
    import('@/lib/emojis').then(({ EMOJIS }) => {
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
    <div className="w-80 p-3">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-xs font-bold tracking-wider text-muted-foreground uppercase">Emoji</span>
        {onRemove ? (
          <Button type="button" variant="ghost" size="sm" onClick={onRemove}>
            Remove
          </Button>
        ) : null}
      </div>
      <InputGroup className="mb-2 bg-muted/40 dark:bg-muted/40">
        <InputGroupAddon>
          <Search className="size-3.5" />
        </InputGroupAddon>
        <InputGroupInput
          value={query}
          placeholder="Search emoji"
          autoFocus
          aria-label="Search emoji"
          className="text-sm font-medium text-foreground"
          onChange={(e) => setQuery(e.target.value)}
        />
      </InputGroup>
      <div className="max-h-72 overflow-y-auto pr-0.5">
        {customEmojis.filter((e) => !query.trim() || e.name.includes(query.trim().toLowerCase())).length > 0 ? (
          <section className="mb-3">
            <div className={groupLabelClass}>Custom</div>
            <div className={gridClass}>
              {customEmojis
                .filter((e) => !query.trim() || e.name.includes(query.trim().toLowerCase()))
                .map((custom) => (
                  <Button key={custom.id} variant="ghost" size="icon" className={gridButtonClass} title={`:${custom.name}:`} onClick={() => onPick(`:${custom.name}:`)}>
                    <img className="size-6 object-contain" src={custom.url} alt={`:${custom.name}:`} />
                  </Button>
                ))}
            </div>
          </section>
        ) : null}
        {emojis.length === 0 ? (
          <div className="py-6 text-center text-sm font-medium text-muted-foreground">Loading emoji...</div>
        ) : groups.length > 0 ? (
          groups.map((group) => (
            <section key={group.category} className="mb-3">
              <div className={groupLabelClass}>{group.category}</div>
              <div className={gridClass}>
                {group.items.map(({ emoji, name }) => (
                  <Button key={`${emoji}-${name}`} variant="ghost" size="icon" className={gridButtonClass} title={name} onClick={() => onPick(emoji)}>
                    {emoji}
                  </Button>
                ))}
              </div>
            </section>
          ))
        ) : (
          <div className="py-6 text-center text-sm font-medium text-muted-foreground">No emoji found</div>
        )}
      </div>
    </div>
  )
}
