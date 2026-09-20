import { ArrowRight, DocumentText as FileText, Trash as Trash2 } from 'reicon-react'
import { Button } from '@/components/ui/button'
import type { Doc, DocBlock } from '@/mock/types'

export function PageBlock({
  block,
  docs,
  onOpen,
  onRemove,
}: {
  block: DocBlock
  docs: Doc[]
  onOpen: (id: string) => void
  onRemove: () => void
}) {
  const page = docs.find((doc) => doc.id === block.refId)

  return (
    <div className="group/pageblock mx-[-6px] flex min-h-[34px] items-center gap-1 rounded-md hover:bg-foreground/[0.02]">
      <Button
        type="button"
        variant="ghost"
        className="flex h-auto min-w-0 flex-1 items-center justify-start gap-[9px] rounded-none border-0 p-1.5 text-[length:inherit] leading-[inherit] font-medium text-foreground underline hover:bg-transparent hover:text-foreground disabled:pointer-events-auto disabled:opacity-100 dark:hover:bg-transparent decoration-border underline-offset-[3px] [&>svg:last-child]:ml-auto [&>svg:last-child]:text-muted-foreground/70 [&>svg:last-child]:opacity-0 group-hover/pageblock:[&>svg:last-child]:opacity-100 focus-visible:[&>svg:last-child]:opacity-100"
        disabled={!page}
        onClick={() => page && onOpen(page.id)}
      >
        <FileText className="size-[18px]" />
        <span className="truncate">{page?.title || block.text || 'Untitled'}</span>
        <ArrowRight className="size-[15px]" />
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        className="invisible mr-1 shrink-0 group-hover/pageblock:visible group-focus-within/pageblock:visible"
        aria-label="Remove page link"
        onClick={onRemove}
      >
        <Trash2 className="size-[14px]" />
      </Button>
    </div>
  )
}
