import type { ComponentProps } from 'react'
import { mergeProps } from '@base-ui/react/merge-props'
import { useRender } from '@base-ui/react/use-render'
import { cn } from 'cn'

/** A full-height main content pane (a page's main column). */
function Pane({ className, ...props }: ComponentProps<'section'>) {
  return <section data-slot="pane" className={cn('flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background', className)} {...props} />
}

/** The 48px header row on top of a pane; it loses its bottom border on phones. */
function PaneHeader({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="pane-header" className={cn('flex min-h-12 shrink-0 items-center gap-2 border-b px-3 py-2 max-[899px]:border-b-0', className)} {...props} />
}

/** The pane's title in its header. A `<span>` by default; `render={<h1 />}` where the title is the page heading. */
function PaneTitle({ className, render, ...props }: useRender.ComponentProps<'span'>) {
  return useRender({
    defaultTagName: 'span',
    props: mergeProps<'span'>({ className: cn('truncate text-[13px] font-semibold text-foreground', className) }, props),
    render,
    state: { slot: 'pane-title' },
  })
}

export { Pane, PaneHeader, PaneTitle }
