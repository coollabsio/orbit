import { useState } from 'react'
import type { EditorViews } from '@orbit/editor/schema'
import { formulaHtml } from '@/components/common/formulaHtml'

/** A formula inside a sentence. A click opens the LaTeX source. */
export const MathInlineView: EditorViews['math']['render'] = ({ inlineContent, updateInlineContent }) => {
  const latex = inlineContent.props.latex
  const [editing, setEditing] = useState(latex.length === 0)
  const [draft, setDraft] = useState(latex)

  function open() {
    setDraft(latex)
    setEditing(true)
  }

  function commit(next = draft) {
    const value = next.trim()
    if (value !== latex) updateInlineContent({ type: 'math', props: { latex: value } })
    setEditing(false)
  }

  if (editing) {
    return (
      <span contentEditable={false} data-slot="inline-math">
        <input
          autoFocus
          aria-label="Formula"
          value={draft}
          size={Math.max(draft.length, 8)}
          className="rounded border border-border bg-background px-1 align-middle font-mono text-[0.9em] outline-none"
          onChange={(event) => setDraft(event.target.value)}
          onBlur={() => commit()}
          onMouseDown={(event) => event.stopPropagation()}
          onKeyDown={(event) => {
            event.stopPropagation()
            if (event.key === 'Enter') {
              event.preventDefault()
              commit(event.currentTarget.value)
            }
            if (event.key === 'Escape') {
              event.preventDefault()
              setDraft(latex)
              setEditing(false)
            }
          }}
        />
      </span>
    )
  }

  const formula = formulaHtml(latex, false)
  if (!latex.trim() || 'error' in formula) {
    return (
      <span
        contentEditable={false}
        data-slot="inline-math"
        className="cursor-text rounded bg-muted px-1 align-middle text-sm text-muted-foreground"
        onMouseDown={(event) => {
          event.preventDefault()
          open()
        }}
      >
        {latex.trim() || 'formula'}
      </span>
    )
  }

  return (
    <span
      contentEditable={false}
      data-slot="inline-math"
      className="mx-0.5 inline-block cursor-text align-middle text-foreground [&_.katex]:text-[1em]"
      onMouseDown={(event) => {
        event.preventDefault()
        open()
      }}
      // KaTeX HTML for this source. `trust: false` blocks commands that load a URL.
      dangerouslySetInnerHTML={{ __html: formula.html }}
    />
  )
}
