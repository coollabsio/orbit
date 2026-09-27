import { ArrowRight, DocumentText } from 'reicon-react'
import { cn } from 'cn'
import { Emoji } from '@/components/common/Emoji'
import { pageBlockState, usePageEditorContext } from './pageEditorContext'

/** Card body of the custom `page` block: links to another page, greyed out when the target is trashed or missing. */
export function PageBlockCard({ pageId }: { pageId: string }) {
  const { resolvePage, onOpenPage } = usePageEditorContext()
  const state = pageBlockState(pageId, resolvePage)
  const disabled = state.kind !== 'ok'
  const icon = state.kind === 'missing' ? null : state.icon

  return (
    <button
      type="button"
      contentEditable={false}
      data-page-id={pageId}
      data-state={state.kind}
      aria-disabled={disabled || undefined}
      className={cn(
        'group/pagecard flex w-full min-w-0 items-center gap-[9px] rounded-md px-1.5 py-1 text-left font-medium text-foreground outline-none select-none',
        'focus-visible:ring-2 focus-visible:ring-ring/50',
        disabled ? 'cursor-default text-muted-foreground' : 'cursor-pointer hover:bg-foreground/[0.04]',
      )}
      onMouseDown={(event) => {
        // Keep ProseMirror from turning the click into a node selection / drag start.
        event.stopPropagation()
      }}
      onClick={(event) => {
        event.preventDefault()
        event.stopPropagation()
        if (!disabled) onOpenPage(pageId)
      }}
    >
      <span className="flex size-[18px] shrink-0 items-center justify-center" aria-hidden>
        {icon ? <Emoji value={icon} size={17} /> : <DocumentText className="size-[18px]" />}
      </span>
      <span
        className={cn(
          'truncate underline decoration-border underline-offset-[3px]',
          state.kind === 'trashed' && 'line-through decoration-muted-foreground/60',
          state.kind === 'missing' && 'no-underline',
        )}
      >
        {state.kind === 'missing' ? 'Missing page' : state.title}
      </span>
      {state.kind === 'trashed' ? (
        <span className="shrink-0 text-xs font-normal text-muted-foreground">Page in trash</span>
      ) : null}
      {state.kind === 'ok' ? (
        <ArrowRight className="ml-auto size-[15px] shrink-0 text-muted-foreground/70 opacity-0 group-hover/pagecard:opacity-100 group-focus-visible/pagecard:opacity-100" />
      ) : null}
    </button>
  )
}

/** Clipboard / external HTML for a `page` block: the target title as plain text. */
export function PageBlockExternalHTML({ pageId }: { pageId: string }) {
  const { resolvePage } = usePageEditorContext()
  const state = pageBlockState(pageId, resolvePage)
  return <p data-page-id={pageId}>{state.kind === 'missing' ? 'Missing page' : state.title}</p>
}

type PageBlockViewProps = { block: { props: { pageId: string } } }

/** `render` of the page block spec. */
export function PageBlockView({ block }: PageBlockViewProps) {
  return <PageBlockCard pageId={block.props.pageId} />
}

/** `toExternalHTML` of the page block spec. */
export function PageBlockExternalView({ block }: PageBlockViewProps) {
  return <PageBlockExternalHTML pageId={block.props.pageId} />
}
