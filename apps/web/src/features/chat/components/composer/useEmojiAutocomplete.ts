import { useState, type KeyboardEvent, type RefObject } from 'react'
import { emojiNamed, loadEmojiIndex, searchEmojiIndex, type EmojiShortcode } from '../../lib/emojiSearch'

/** `:na` at the start of the text or after a space: two characters open the list, as in Discord. */
const OPEN = /(?:^|\s):([a-z0-9_+-]{2,})$/i
/** `:name:` typed in full. */
const COMPLETE = /(?:^|\s):([a-z0-9_+-]+):$/i

interface EmojiQuery {
  /** Where the `:` is. */
  start: number
  end: number
  query: string
}

/**
 * Emoji by name in a text field: `:jo` opens a list (↑/↓, Enter or Tab inserts, Esc closes), and `:joy:` typed in
 * full becomes the emoji at once. The emoji data loads the first time a `:` is typed.
 */
export function useEmojiAutocomplete(text: string, setText: (next: string) => void, inputRef: RefObject<HTMLTextAreaElement | null>) {
  const [index, setIndex] = useState<EmojiShortcode[] | null>(null)
  const [state, setState] = useState<EmojiQuery | null>(null)
  const [activeIndex, setActiveIndex] = useState(0)

  const suggestions = state && index ? searchEmojiIndex(index, state.query) : []
  const open = suggestions.length > 0

  function replace(from: number, to: number, insertion: string, value: string) {
    const cursor = from + insertion.length
    setText(`${value.slice(0, from)}${insertion}${value.slice(to)}`)
    setState(null)
    requestAnimationFrame(() => {
      const element = inputRef.current
      if (!element) return
      element.focus()
      element.setSelectionRange(cursor, cursor)
    })
  }

  /** After the text or the caret changed. */
  function update(value: string, cursor: number) {
    const before = value.slice(0, cursor)
    if (index === null && before.includes(':')) void loadEmojiIndex().then(setIndex, () => {})
    const match = OPEN.exec(before)
    if (!match) return setState(null)
    setState({ start: cursor - match[1].length - 1, end: cursor, query: match[1] })
    setActiveIndex(0)
  }

  /** After the user typed: turns a `:name:` that ends at the caret into its emoji. Returns true when it did. */
  function complete(value: string, cursor: number): boolean {
    const match = index ? COMPLETE.exec(value.slice(0, cursor)) : null
    const emoji = match && index ? emojiNamed(index, match[1]) : null
    if (!match || !emoji) return false
    // no space after it: the user types the next character
    replace(cursor - match[1].length - 2, cursor, emoji, value)
    return true
  }

  function insert(suggestion: EmojiShortcode) {
    if (state) replace(state.start, state.end, `${suggestion.emoji} `, text)
  }

  /** Returns true when the list used the key. */
  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>): boolean {
    // During IME composition the keys belong to the IME.
    if (!open || event.nativeEvent.isComposing) return false
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const step = event.key === 'ArrowDown' ? 1 : -1
      setActiveIndex((current) => (current + step + suggestions.length) % suggestions.length)
      return true
    }
    if (event.key === 'Enter' || event.key === 'Tab') {
      event.preventDefault()
      insert(suggestions[activeIndex] ?? suggestions[0])
      return true
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      setState(null)
      return true
    }
    return false
  }

  return { open, query: state?.query ?? '', suggestions, activeIndex, setActiveIndex, update, complete, insert, close: () => setState(null), handleKeyDown }
}
