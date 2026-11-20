import { useState, type ClipboardEvent, type KeyboardEvent } from 'react'
import { CloseCircle } from 'reicon-react'
import { cn } from 'cn'
import { Badge } from '@/components/ui/badge'

/** The new values in `text` (split on commas and white space), without ones already in `values`. */
export function addTags(values: string[], text: string): string[] {
  const next = [...values]
  for (const part of text.split(/[\s,]+/)) {
    const value = part.trim()
    if (value && !next.includes(value)) next.push(value)
  }
  return next
}

/**
 * A list of short values as removable badges. Enter, a comma or a space adds what you typed; Backspace in the empty
 * box removes the last one. Text left in the box is added when it loses focus, so a save never drops it.
 */
export function TagInput({
  id,
  values,
  onChange,
  placeholder,
  className,
}: {
  id?: string
  values: string[]
  onChange: (values: string[]) => void
  placeholder?: string
  className?: string
}) {
  const [draft, setDraft] = useState('')

  function commit(text = draft) {
    setDraft('')
    const next = addTags(values, text)
    if (next.length !== values.length) onChange(next)
  }

  function keyDown(event: KeyboardEvent<HTMLInputElement>) {
    if ((event.key === 'Enter' || event.key === ',' || event.key === ' ') && draft.trim() !== '') {
      event.preventDefault()
      commit()
    } else if (event.key === 'Backspace' && draft === '' && values.length > 0) {
      onChange(values.slice(0, -1))
    }
  }

  function paste(event: ClipboardEvent<HTMLInputElement>) {
    const text = event.clipboardData.getData('text')
    if (!/[\s,]/.test(text.trim())) return
    event.preventDefault()
    commit(draft + text)
  }

  return (
    <div
      className={cn(
        'flex min-h-8 w-full min-w-0 flex-wrap items-center gap-1 rounded-lg border border-input bg-transparent px-1.5 py-1 transition-colors focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50 dark:bg-input/30',
        className,
      )}
    >
      {values.map((value) => (
        <Badge key={value} variant="secondary" className="gap-1 pr-1 font-mono">
          {value}
          <button
            type="button"
            aria-label={`Remove ${value}`}
            className="rounded-full text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
            onClick={() => onChange(values.filter((item) => item !== value))}
          >
            <CloseCircle aria-hidden className="size-3.5" />
          </button>
        </Badge>
      ))}
      <input
        id={id}
        autoComplete="off"
        className="h-6 min-w-24 flex-1 bg-transparent px-1 text-base text-foreground outline-none placeholder:text-muted-foreground md:text-sm"
        placeholder={values.length === 0 ? placeholder : undefined}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={keyDown}
        onPaste={paste}
        onBlur={() => commit()}
      />
    </div>
  )
}
