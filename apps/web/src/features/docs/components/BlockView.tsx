import { cx } from '../../../lib/cx'
import type { DocBlock } from '../../../mock/types'
import { numberedIndex } from '../lib'
import type { MentionToken } from '../../chat/chatLib'
import { mentionifyText } from '../../chat/markdown'

interface BlockViewProps {
  block: DocBlock
  blocks: DocBlock[]
  index: number
  mentionTokens: MentionToken[]
  onEdit: () => void
  onToggleTodo: () => void
}

export function BlockView({ block, blocks, index, mentionTokens, onEdit, onToggleTodo }: BlockViewProps) {
  const empty = block.text.trim() === ''
  const text = empty ? 'Empty block' : mentionifyText(block.text, `block-${block.id}`, mentionTokens)

  const inner = () => {
    switch (block.type) {
      case 'h1':
      case 'h2':
      case 'h3':
      case 'p':
        return <span className={cx(empty && 'doc-block-empty')}>{text}</span>
      case 'bullet':
        return (
          <span className="doc-block-list">
            <span className="doc-block-marker">•</span>
            <span className={cx(empty && 'doc-block-empty')}>{text}</span>
          </span>
        )
      case 'numbered':
        return (
          <span className="doc-block-list">
            <span className="doc-block-marker">{numberedIndex(blocks, index)}.</span>
            <span className={cx(empty && 'doc-block-empty')}>{text}</span>
          </span>
        )
      case 'quote':
        return (
          <span className="doc-block-quote" style={{ display: 'block' }}>
            <span className={cx(empty && 'doc-block-empty')}>{text}</span>
          </span>
        )
      case 'code':
        return <pre className="doc-block-code">{block.text || ' '}</pre>
      case 'divider':
        return <hr className="doc-block-divider" />
      case 'todo':
        return (
          <span className="doc-block-todo" data-checked={block.checked === true}>
            <input
              type="checkbox"
              checked={block.checked === true}
              aria-label="Toggle to-do"
              onClick={(e) => e.stopPropagation()}
              onChange={onToggleTodo}
            />
            <span className={cx('doc-block-todo-text', empty && 'doc-block-empty')}>{text}</span>
          </span>
        )
    }
  }

  return (
    <div
      className={cx(
        'doc-block',
        block.type === 'h1' && 'doc-block-h1',
        block.type === 'h2' && 'doc-block-h2',
        block.type === 'h3' && 'doc-block-h3',
      )}
      onClick={onEdit}
    >
      {inner()}
    </div>
  )
}
