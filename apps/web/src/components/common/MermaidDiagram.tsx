// A Mermaid diagram drawn from its source text (code blocks of tasks, comments, chat and docs). The library is large,
// so it loads only when the first diagram shows.
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'

function subscribeTheme(onChange: () => void) {
  const observer = new MutationObserver(onChange)
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
  return () => observer.disconnect()
}

// The theme comes from the document, not from `ThemeContext`: the docs editor draws the diagram in a React root of its own.
const isDark = () => document.documentElement.classList.contains('dark')

let stylesBlocked: boolean | undefined

/** The production CSP (`style-src 'self'`) blocks a runtime `<style>`; a blocked one has no sheet. */
function inlineStylesBlocked() {
  if (stylesBlocked === undefined) {
    const probe = document.createElement('style')
    document.head.appendChild(probe)
    stylesBlocked = probe.sheet === null
    probe.remove()
  }
  return stylesBlocked
}

function setAdopted(sheet: CSSStyleSheet, adopted: boolean) {
  const others = document.adoptedStyleSheets.filter((item) => item !== sheet)
  document.adoptedStyleSheets = adopted ? [...others, sheet] : others
}

interface Drawing {
  svg: string
  /** The style rules of the SVG where the CSP blocks its `<style>`, else empty. */
  css: string
}

let diagramCount = 0
/**
 * The last drawings, by theme and source. A chat message that scrolls back into view shows its diagram at once and
 * at its full height. Two copies of one drawing on the page have the same id; their rules and markers are the same.
 */
const drawings = new Map<string, Drawing>()

/**
 * Takes the `<style>` elements out of an SVG. The parser, not a pattern, reads them: the sanitized SVG writes `>` in
 * a selector as `&gt;`.
 */
function splitStyles(svg: string): Drawing {
  const template = document.createElement('template')
  template.innerHTML = svg
  const styles = [...template.content.querySelectorAll('style')]
  const css = styles.map((style) => style.textContent).join('\n')
  for (const style of styles) style.remove()
  return { svg: template.innerHTML, css }
}

/**
 * Mermaid puts the colours and font sizes of a diagram in a `<style>` inside the SVG. Where the CSP blocks that, the
 * same rules (they are scoped to the id of the SVG) go into a constructed style sheet, which the CSP allows. The
 * diagram is then drawn again with that sheet, because Mermaid measured the text of the first drawing without its styles.
 */
async function renderDiagram(code: string, dark: boolean): Promise<Drawing> {
  const key = `${dark}:${code}`
  const cached = drawings.get(key)
  if (cached) return cached

  const { default: mermaid } = await import('mermaid')
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    // an error is the message below the diagram, not Mermaid's own picture in the page
    suppressErrorRendering: true,
    theme: dark ? 'dark' : 'default',
    fontFamily: getComputedStyle(document.body).fontFamily,
  })
  await mermaid.parse(code)
  const id = `mermaid-${++diagramCount}`
  let drawing: Drawing = { svg: (await mermaid.render(id, code)).svg, css: '' }
  if (inlineStylesBlocked()) {
    const sheet = new CSSStyleSheet()
    sheet.replaceSync(splitStyles(drawing.svg).css)
    setAdopted(sheet, true)
    try {
      drawing = splitStyles((await mermaid.render(id, code)).svg)
    } finally {
      setAdopted(sheet, false)
    }
  }
  drawings.set(key, drawing)
  if (drawings.size > 50) drawings.delete(drawings.keys().next().value!)
  return drawing
}

export function MermaidDiagram({ code }: { code: string }) {
  const dark = useSyncExternalStore(subscribeTheme, isDark)
  const source = code.trim()
  const [drawing, setDrawing] = useState(() => drawings.get(`${dark}:${source}`))
  const [error, setError] = useState('')
  const drawn = useRef(false)
  const svg = drawing?.svg

  useEffect(() => {
    let cancelled = false
    // The first drawing starts at once. A change of the text (the docs editor, the task preview) waits for a pause in typing.
    const timer = setTimeout(
      () => {
        if (!source) {
          setDrawing(undefined)
          setError('')
          return
        }
        renderDiagram(source, dark).then(
          (result) => {
            if (cancelled) return
            drawn.current = true
            setDrawing(result)
            setError('')
          },
          (cause: unknown) => {
            // A parse error ends with a long "Expecting …" list of token names, and an unknown diagram type with the
            // whole text; the first part is enough.
            if (!cancelled) setError(cause instanceof Error ? cause.message.replace(/(\nExpecting | for text: )[\s\S]*$/, '') : 'Could not draw the diagram.')
          },
        )
      },
      drawn.current ? 300 : 0,
    )
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [source, dark])

  // Before paint, so a new diagram never shows without its rules. The old diagram keeps its rules until then.
  const css = drawing?.css
  useLayoutEffect(() => {
    if (!css) return
    const sheet = new CSSStyleSheet()
    sheet.replaceSync(css)
    setAdopted(sheet, true)
    return () => setAdopted(sheet, false)
  }, [css])

  return (
    <div data-slot="mermaid-diagram" className="max-w-full overflow-x-auto">
      {svg ? (
        // Mermaid's own output; with `securityLevel: 'strict'` it is sanitized (DOMPurify) and has no scripts or click handlers.
        <div role="img" aria-label="Diagram" className="[&>svg]:mx-auto [&>svg]:block [&>svg]:h-auto" dangerouslySetInnerHTML={{ __html: svg }} />
      ) : !error ? (
        <p className="text-xs text-muted-foreground">{source ? 'Drawing the diagram…' : 'The diagram is empty.'}</p>
      ) : null}
      {error ? (
        <p role="alert" className="font-mono text-xs whitespace-pre-wrap text-destructive">
          {error}
        </p>
      ) : null}
    </div>
  )
}
