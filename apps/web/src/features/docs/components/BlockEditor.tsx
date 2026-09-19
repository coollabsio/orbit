import { useEffect, useRef, useState } from 'react'
import { cx } from '../../../lib/cx'
import type { DocBlock, User } from '../../../mock/types'
import { clipboardFiles } from '../../chat/attachmentLib'
import { MentionPopover } from '../../chat/components/MentionPopover'
import { useMentionAutocomplete } from '../../chat/useMentionAutocomplete'
import { SLASH_ITEMS, filterSuggestionItems, getSlashState, type SlashItem } from '../slashItems'
import { SlashMenu } from './SlashMenu'

interface BlockEditorProps {
  block: DocBlock
  /** Workspace members for @mention autocomplete (same popup as chat). */
  users: User[]
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
  /** "/image" and "/file" open a file picker for this block. */
  onPickMedia: (kind: 'image' | 'file') => void
  /** Pasted files become image/file blocks after this one. */
  onFiles: (files: File[]) => void
}

export function BlockEditor({
  block,
  users,
  onCommit,
  onDelete,
  onChangeType,
  onToggleTodo,
  marker,
  autoFocus,
  onSubpage,
  onLinkPage,
  onPickMedia,
  onFiles,
}: BlockEditorProps) {
  const [text, setText] = useState(block.text)
  const [slash, setSlash] = useState<{ start: number; query: string } | null>(null)
  const [slashIndex, setSlashIndex] = useState(0)
  const ref = useRef<HTMLTextAreaElement>(null)
  const suppressBlurRef = useRef(false)

  const grow = (el: HTMLTextAreaElement) => {
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
  }

  const mention = useMentionAutocomplete(users, text, setText, ref, () => {
    if (ref.current) grow(ref.current)
  })

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
      case 'media':
        if (item.action.media === 'embed') {
          setText('')
          onChangeType('embed', '')
          requestAnimationFrame(() => {
            const el = ref.current
            if (!el) return
            el.focus()
            grow(el)
          })
        } else {
          suppressBlurRef.current = true
          onPickMedia(item.action.media)
        }
        return
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
    if (!menuOpen && mention.handleKeyDown(e)) return
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
        'group/block relative mx-[-6px] flex min-w-0 items-start gap-1.5 rounded-md bg-transparent px-1.5 py-0.5 first:mt-0 hover:bg-transparent max-[899px]:py-px',
        block.type === 'h1' && 'mt-[18px] text-[28px] leading-[1.3] font-bold text-foreground max-[899px]:mt-[14px] max-[899px]:text-[21px]',
        block.type === 'h2' && 'mt-[14px] text-[24px] leading-[1.35] font-semibold text-foreground max-[899px]:mt-[11px] max-[899px]:text-[18px]',
        block.type === 'h3' && 'mt-[10px] text-[19px] leading-[1.4] font-semibold text-foreground max-[899px]:mt-[8px] max-[899px]:text-[15px]',
        block.type === 'code' &&
          'my-1 rounded-lg px-3.5 py-3 font-mono text-[13px] leading-[1.6] max-[899px]:px-2.5 max-[899px]:py-[9px] max-[899px]:text-[11px] max-[899px]:leading-4',
        block.type === 'quote' && 'my-1 border-l-[3px] border-muted pl-3.5 text-muted-foreground italic max-[899px]:pl-2.5',
      )}
    >
      {block.type === 'todo' ? (
        <input
          type="checkbox"
          className="mt-[5px] size-[15px] shrink-0 accent-primary"
          checked={block.checked === true}
          aria-label="Toggle to-do"
          onChange={onToggleTodo}
        />
      ) : marker ? (
        <span className="min-w-3 shrink-0 select-none text-right text-muted-foreground">{marker}</span>
      ) : null}
      <textarea
        ref={ref}
        className="block w-full min-w-0 flex-1 resize-none overflow-hidden border-none bg-transparent p-0 [font:inherit] text-inherit outline-none placeholder:text-transparent focus:placeholder:text-muted-foreground/70 focus:outline-none max-[767px]:[font-size:inherit]!"
        rows={1}
        value={text}
        placeholder={block.type === 'embed' ? 'Paste a link and press Enter…' : block.type === 'divider' ? 'Divider' : "Type '/' for commands…"}
        onChange={(e) => {
          suppressBlurRef.current = false
          setText(e.target.value)
          updateSlash(e.target.value, e.target.selectionStart)
          mention.update(e.target.value, e.target.selectionStart)
          grow(e.target)
        }}
        onClick={(e) => {
          updateSlash(text, e.currentTarget.selectionStart)
          mention.update(text, e.currentTarget.selectionStart)
        }}
        onSelect={(e) => {
          updateSlash(text, e.currentTarget.selectionStart)
          mention.update(text, e.currentTarget.selectionStart)
        }}
        onKeyDown={onKeyDown}
        onFocus={() => {
          suppressBlurRef.current = false
        }}
        onBlur={() => {
          // clicking outside must close the slash menu and the mention popup too
          setSlash(null)
          mention.close()
          finish('close')
        }}
        onPaste={(e) => {
          const files = clipboardFiles(e)
          if (files.length === 0) return
          e.preventDefault()
          onFiles(files)
        }}
      />
      {!menuOpen && mention.open ? (
        <MentionPopover
          placement="below"
          showHandle={false}
          suggestions={mention.suggestions}
          activeIndex={mention.activeIndex}
          onSelect={mention.insert}
          onHover={mention.setActiveIndex}
        />
      ) : null}
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
