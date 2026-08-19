import { useEffect, useRef, useState } from 'react'
import { cx } from '../../../lib/cx'
import type { DocBlock } from '../../../mock/types'
import { SLASH_ITEMS, filterSuggestionItems, getSlashState, type SlashItem } from '../slashItems'
import { SlashMenu } from './SlashMenu'

interface BlockEditorProps {
  block: DocBlock
  /** Commit the text; 'insert' also creates a paragraph after this block. */
  onCommit: (text: string, action: 'close' | 'insert') => void
  onDelete: () => void
  onChangeType: (type: DocBlock['type'], text: string) => void
  onToggleTodo: () => void
  marker?: string
  autoFocus?: boolean
  /** The docs reference slash actions: create a sub-page / open the link-a-page dialog. */
  onSubpage: (textWithoutSlash: string) => void
  onLinkPage: (textWithoutSlash: string) => void
}

export function BlockEditor({
  block,
  onCommit,
  onDelete,
  onChangeType,
  onToggleTodo,
  marker,
  autoFocus,
  onSubpage,
  onLinkPage,
}: BlockEditorProps) {
  const [text, setText] = useState(block.text)
  const [slash, setSlash] = useState<{ start: number; query: string } | null>(null)
  const [slashIndex, setSlashIndex] = useState(0)
  const ref = useRef<HTMLTextAreaElement>(null)
  const suppressBlurRef = useRef(false)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    if (autoFocus) {
      el.focus()
      el.setSelectionRange(el.value.length, el.value.length)
    }
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
  }, [autoFocus])

  const grow = (el: HTMLTextAreaElement) => {
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
  }

  const finish = (action: 'close' | 'insert') => {
    if (suppressBlurRef.current) return
    if (action === 'insert') suppressBlurRef.current = true
    onCommit(text, action)
  }

  const slashResults = slash ? filterSuggestionItems(SLASH_ITEMS, slash.query) : []
  const menuOpen = slash !== null && slashResults.length > 0

  const updateSlash = (value: string, cursor: number) => {
    const state = getSlashState(value, cursor)
    setSlash(state)
    if (state?.query !== slash?.query) setSlashIndex(0)
  }

  /** The block text with the "/query" trigger removed (the reference editor strips it on select). */
  const textWithoutSlash = () => {
    if (!slash) return text
    const cursor = ref.current?.selectionStart ?? text.length
    return text.slice(0, slash.start) + text.slice(cursor)
  }

  const selectSlashItem = (item: SlashItem) => {
    const stripped = textWithoutSlash()
    setSlash(null)
    switch (item.action.kind) {
      case 'turn-into': {
        if (item.action.type === 'divider') {
          // a divider has no text: turn into divider and continue in a fresh paragraph
          suppressBlurRef.current = true
          onChangeType('divider', '')
          onCommit('', 'insert')
          return
        }
        setText(stripped)
        onChangeType(item.action.type, stripped)
        requestAnimationFrame(() => {
          const el = ref.current
          if (!el) return
          el.focus()
          el.setSelectionRange(stripped.length, stripped.length)
          grow(el)
        })
        return
      }
      case 'subpage':
        suppressBlurRef.current = true
        onSubpage(stripped)
        return
      case 'link-page':
        suppressBlurRef.current = true
        onLinkPage(stripped)
        return
    }
  }

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (menuOpen) {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setSlashIndex((i) => (i + 1) % slashResults.length)
        return
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault()
        setSlashIndex((i) => (i - 1 + slashResults.length) % slashResults.length)
        return
      }
      if (e.key === 'Enter') {
        e.preventDefault()
        selectSlashItem(slashResults[slashIndex])
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        setSlash(null)
        return
      }
    }
    if (e.key === 'Enter' && !e.shiftKey && block.type !== 'code') {
      e.preventDefault()
      finish('insert')
    } else if (e.key === 'Escape') {
      e.preventDefault()
      finish('close')
    } else if (e.key === 'Backspace' && text === '') {
      e.preventDefault()
      suppressBlurRef.current = true
      onDelete()
    }
  }

  return (
    <div
      className={cx(
        'doc-block doc-block-editing',
        block.type === 'h1' && 'doc-block-h1',
        block.type === 'h2' && 'doc-block-h2',
        block.type === 'h3' && 'doc-block-h3',
        block.type === 'code' && 'doc-block-code',
        block.type === 'quote' && 'doc-block-quote',
      )}
      style={{ position: 'relative' }}
    >
      {block.type === 'todo' ? (
        <input
          type="checkbox"
          checked={block.checked === true}
          aria-label="Toggle to-do"
          onChange={onToggleTodo}
        />
      ) : marker ? (
        <span className="doc-block-marker">{marker}</span>
      ) : null}
      <textarea
        ref={ref}
        className="doc-block-textarea"
        rows={1}
        value={text}
        placeholder={block.type === 'divider' ? 'Divider' : "Type '/' for commands…"}
        onChange={(e) => {
          suppressBlurRef.current = false
          setText(e.target.value)
          updateSlash(e.target.value, e.target.selectionStart)
          grow(e.target)
        }}
        onClick={(e) => updateSlash(text, e.currentTarget.selectionStart)}
        onSelect={(e) => updateSlash(text, e.currentTarget.selectionStart)}
        onKeyDown={onKeyDown}
        onFocus={() => {
          suppressBlurRef.current = false
        }}
        onBlur={() => finish('close')}
      />
      {menuOpen ? (
        <SlashMenu
          items={slashResults}
          selectedIndex={slashIndex}
          onSelect={selectSlashItem}
          onHover={setSlashIndex}
        />
      ) : null}
    </div>
  )
}
