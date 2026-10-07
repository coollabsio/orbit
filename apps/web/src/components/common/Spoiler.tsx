import { useState, type ReactNode } from 'react'

/** `||text||` in markdown: a dark bar until the reader clicks it or presses Enter or Space on it, then the text. */
export function Spoiler({ children }: { children: ReactNode }) {
  const [revealed, setRevealed] = useState(false)

  return (
    <span
      data-slot="spoiler"
      data-revealed={revealed ? '' : undefined}
      role={revealed ? undefined : 'button'}
      tabIndex={revealed ? undefined : 0}
      aria-label={revealed ? undefined : 'Spoiler'}
      className="cursor-pointer rounded-sm bg-foreground/80 text-transparent select-none data-revealed:cursor-auto data-revealed:bg-muted data-revealed:text-inherit data-revealed:select-auto"
      onClick={() => setRevealed(true)}
      onKeyDown={(e) => {
        if (revealed || (e.key !== 'Enter' && e.key !== ' ')) return
        // Space would scroll the page
        e.preventDefault()
        setRevealed(true)
      }}
    >
      {/* a link, a mention or an emoji has its own colour, so transparent text alone would not hide it */}
      <span data-hidden={revealed ? undefined : ''} className="data-hidden:invisible">
        {children}
      </span>
    </span>
  )
}
