import { createElement } from 'react'
import { cn } from 'cn'
import { Tip } from '@/components/common/Tip'

/** A title with help text: a solid underline (as in Coolify) and the text in a tooltip on hover or focus. */
export function TitleTip({ title, text, as = 'h3', className }: { title: string; text: string; as?: 'h1' | 'h2' | 'h3' | 'h4'; className?: string }) {
  return (
    <>
      <Tip label={text} side="bottom">
        {createElement(as, { tabIndex: 0, className: cn('w-fit cursor-pointer underline underline-offset-4', className) }, title)}
      </Tip>
      <p className="sr-only">{text}</p>
    </>
  )
}
