import { useEffect, useState } from 'react'
import type { Formula } from './formulaHtml'

/** A display formula. The drawer loads only when a formula is on screen. */
export function MathFormula({ code }: { code: string }) {
  const [formula, setFormula] = useState<Formula | null>(null)

  useEffect(() => {
    let cancelled = false
    void import('./formulaHtml').then(({ formulaHtml }) => {
      if (!cancelled) setFormula(formulaHtml(code))
    })
    return () => {
      cancelled = true
    }
  }, [code])

  if (!code.trim()) return <p className="text-center text-sm text-muted-foreground">Empty formula</p>
  if (!formula || ('html' in formula && !formula.html)) return <div className="min-h-8" />
  if ('error' in formula) return <p className="text-center text-sm text-destructive">{formula.error}</p>
  return (
    <div
      data-slot="math-formula"
      className="overflow-x-auto text-center text-foreground [&_.katex-display]:my-0"
      // KaTeX HTML for this source. `trust: false` blocks commands that load a URL.
      dangerouslySetInnerHTML={{ __html: formula.html }}
    />
  )
}
