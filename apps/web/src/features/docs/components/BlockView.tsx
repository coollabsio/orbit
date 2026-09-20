import { Checkbox } from '@/components/ui/checkbox'
import { Separator } from '@/components/ui/separator'
import { cn } from 'cn'
import type { DocBlock } from '@/mock/types'
import { numberedIndex } from '@/features/docs/docsLib'
import type { MentionToken } from '@/lib/mentions'
import { mentionifyText } from '@/lib/markdown'

interface BlockViewProps {
  block: DocBlock
  blocks: DocBlock[]
  index: number
  mentionTokens: MentionToken[]
  onEdit: () => void
  onToggleTodo: () => void
}

const EMPTY = 'text-muted-foreground/70'
const LIST = 'flex items-baseline gap-2.5 max-[899px]:gap-[7px]'
const MARKER = 'min-w-3 shrink-0 select-none text-right text-muted-foreground'

export function BlockView({ block, blocks, index, mentionTokens, onEdit, onToggleTodo }: BlockViewProps) {
  const empty = block.text.trim() === ''
  const text = empty ? 'Empty block' : mentionifyText(block.text, `block-${block.id}`, mentionTokens)

  const inner = () => {
    switch (block.type) {
      case 'h1':
      case 'h2':
      case 'h3':
      case 'p':
        return <span className={cn(empty && EMPTY)}>{text}</span>
      case 'bullet':
        return (
          <span className={LIST}>
            <span className={MARKER}>•</span>
            <span className={cn(empty && EMPTY)}>{text}</span>
          </span>
        )
      case 'numbered':
        return (
          <span className={LIST}>
            <span className={MARKER}>{numberedIndex(blocks, index)}.</span>
            <span className={cn(empty && EMPTY)}>{text}</span>
          </span>
        )
      case 'quote':
        return (
          <span className="my-1 block border-l-[3px] border-muted pl-3.5 text-muted-foreground italic max-[899px]:pl-2.5">
            <span className={cn(empty && EMPTY)}>{text}</span>
          </span>
        )
      case 'code':
        return (
          <pre className="my-1 rounded-lg bg-muted px-3.5 py-3 font-mono text-[13px] leading-[1.6] break-words whitespace-pre-wrap max-[899px]:px-2.5 max-[899px]:py-[9px] max-[899px]:text-[11px] max-[899px]:leading-4">
            {block.text || ' '}
          </pre>
        )
      case 'divider':
        return <Separator className="my-3 max-[899px]:my-[9px]" />
      case 'todo':
        return (
          <span className={cn('group/todo', LIST)} data-checked={block.checked === true}>
            <Checkbox
              className="size-[15px] shrink-0 translate-y-0.5 cursor-pointer"
              checked={block.checked === true}
              aria-label="Toggle to-do"
              onClick={(e) => e.stopPropagation()}
              onCheckedChange={onToggleTodo}
            />
            <span
              className={cn(
                'group-data-[checked=true]/todo:text-muted-foreground/70 group-data-[checked=true]/todo:line-through',
                empty && EMPTY,
              )}
            >
              {text}
            </span>
          </span>
        )
    }
  }

  return (
    <div
      className={cn(
        'group/block relative mx-[-6px] min-w-0 cursor-text rounded-md px-1.5 py-0.5 first:mt-0 hover:bg-foreground/[0.02] max-[899px]:py-px',
        block.type === 'h1' && 'mt-[18px] text-[28px] leading-[1.3] font-bold text-foreground max-[899px]:mt-[14px] max-[899px]:text-[21px]',
        block.type === 'h2' && 'mt-[14px] text-[24px] leading-[1.35] font-semibold text-foreground max-[899px]:mt-[11px] max-[899px]:text-[18px]',
        block.type === 'h3' && 'mt-[10px] text-[19px] leading-[1.4] font-semibold text-foreground max-[899px]:mt-[8px] max-[899px]:text-[15px]',
      )}
      onClick={onEdit}
    >
      {inner()}
    </div>
  )
}
