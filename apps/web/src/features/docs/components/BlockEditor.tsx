import { useEffect, useRef, useState } from 'react'
import { Category } from 'reicon-react'
import { Dropdown } from '../../../components/ui/Dropdown'
import { cx } from '../../../lib/cx'
import type { DocBlock } from '../../../mock/types'
import { BLOCK_TYPES } from '../lib'

interface BlockEditorProps {
  block: DocBlock
  /** Commit the text; 'insert' also creates a paragraph after this block. */
  onCommit: (text: string, action: 'close' | 'insert') => void
  onDelete: () => void
  onChangeType: (type: DocBlock['type'], text: string) => void
}

export function BlockEditor({ block, onCommit, onDelete, onChangeType }: BlockEditorProps) {
  const [text, setText] = useState(block.text)
  const ref = useRef<HTMLTextAreaElement>(null)
  const doneRef = useRef(false)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    el.focus()
    el.setSelectionRange(el.value.length, el.value.length)
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
  }, [])

  const grow = (el: HTMLTextAreaElement) => {
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
  }

  const finish = (action: 'close' | 'insert') => {
    if (doneRef.current) return
    doneRef.current = true
    onCommit(text, action)
  }

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && block.type !== 'code') {
      e.preventDefault()
      finish('insert')
    } else if (e.key === 'Escape') {
      e.preventDefault()
      finish('close')
    } else if (e.key === 'Backspace' && text === '') {
      e.preventDefault()
      doneRef.current = true
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
    >
      <span onMouseDown={(e) => e.preventDefault()}>
        <Dropdown
          trigger={() => (
            <button
              type="button"
              className="icon-button doc-block-type-button"
              aria-label="Change block type"
            >
              <Category size={14} />
            </button>
          )}
        >
          {(close) => (
            <>
              <div className="popover-heading">Turn into</div>
              {BLOCK_TYPES.map((entry) => (
                <button
                  key={entry.type}
                  type="button"
                  className="popover-option"
                  data-selected={entry.type === block.type}
                  onClick={() => {
                    close()
                    onChangeType(entry.type, text)
                  }}
                >
                  {entry.label}
                </button>
              ))}
            </>
          )}
        </Dropdown>
      </span>
      <textarea
        ref={ref}
        className="doc-block-textarea"
        rows={1}
        value={text}
        placeholder={block.type === 'divider' ? 'Divider' : 'Type something…'}
        onChange={(e) => {
          setText(e.target.value)
          grow(e.target)
        }}
        onKeyDown={onKeyDown}
        onBlur={() => finish('close')}
      />
    </div>
  )
}
