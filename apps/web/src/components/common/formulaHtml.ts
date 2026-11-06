// A display formula from LaTeX. KaTeX returns HTML (and MathML for assistive tech). The styles are in index.css.
import katex from 'katex'

export type Formula = { html: string } | { error: string }

/** The formula for `source`, or a short error. Blank source has no formula. `display` is a block formula. */
export function formulaHtml(source: string, display = true): Formula {
  const tex = source.replace(/\n+/g, ' ').trim()
  if (!tex) return { html: '' }
  try {
    return {
      html: katex.renderToString(tex, {
        displayMode: display,
        throwOnError: true,
        // A formula must not run a command or load a URL.
        trust: false,
        strict: 'ignore',
      }),
    }
  } catch (cause) {
    const message = cause instanceof Error ? cause.message.replace(/^KaTeX parse error:\s*/i, '').replace(/\s+/g, ' ').trim() : ''
    return { error: message || 'Could not draw the formula.' }
  }
}
