import { createElement } from 'react'
import { cn } from 'cn'
import { HelpTip } from '@/components/common/HelpTip'

/** A title with help text: a solid underline (as in Coolify) and the text in a popup on hover, focus or tap. */
export function TitleTip({ title, text, as = 'h3', className }: { title: string; text: string; as?: 'h1' | 'h2' | 'h3' | 'h4'; className?: string }) {
  return (
    <>
      <HelpTip text={text} side="bottom">
        {createElement(as, { tabIndex: 0, className: cn('w-fit cursor-pointer underline underline-offset-4', className) }, title)}
      </HelpTip>
      <p className="sr-only">{text}</p>
    </>
  )
}
