// the chat reference CodeBlock (MessageItem.tsx): fenced code with copy button and syntax colors. A `mermaid` block
// shows its diagram; a button switches to the source.
import { Fragment, useState } from 'react'
import { codeLines, detectLanguage, languageId, MERMAID_LANGUAGE } from '@/lib/codeLanguages'
import { Button } from '@/components/ui/button'
import { codeTokenVariants } from './codeToken'
import { CopyCodeButton } from './CopyCodeButton'
import { MermaidDiagram } from './MermaidDiagram'

export function CodeBlock({ code, language = '' }: { code: string; language?: string }) {
  const [showCode, setShowCode] = useState(false)
  const mermaid = (language ? languageId(language) : detectLanguage(code)) === MERMAID_LANGUAGE
  const diagram = mermaid && !showCode

  return (
    <div
      data-diagram={diagram ? '' : undefined}
      className="group relative my-1 w-fit max-w-full data-diagram:w-full not-data-diagram:min-[900px]:max-w-[80%]"
    >
      {diagram ? (
        <div className="rounded-lg border border-border bg-muted/20 px-3 pt-11 pb-3">
          <MermaidDiagram code={code} />
        </div>
      ) : (
        <pre
          data-mermaid={mermaid ? '' : undefined}
          // the source of a diagram has two buttons above it; other code has the copy button at its right
          className="w-full rounded-lg border border-border bg-muted/20 py-3 pr-12 pl-3 font-mono text-[13px] leading-5 font-semibold whitespace-pre-wrap text-foreground/85 [overflow-wrap:anywhere] data-mermaid:pt-11"
        >
          <code className="font-[inherit] whitespace-pre-wrap [overflow-wrap:anywhere]">
            {codeLines(code, language).map((line, lineIndex) => (
              <Fragment key={lineIndex}>
                {lineIndex > 0 ? '\n' : null}
                {line.map((token, index) => {
                  const className = codeTokenVariants({ type: token.type })
                  return className ? (
                    <span key={index} data-token={token.type} className={className}>
                      {token.value}
                    </span>
                  ) : (
                    token.value
                  )
                })}
              </Fragment>
            ))}
          </code>
        </pre>
      )}
      <div className="absolute top-2 right-2 flex gap-1">
        {mermaid ? (
          <Button
            type="button"
            variant="outline"
            className="h-7 bg-background/90 px-2 text-[11px] font-semibold text-muted-foreground shadow-sm transition-opacity duration-150 focus-visible:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100 dark:bg-background/90"
            onClick={() => setShowCode(!showCode)}
          >
            {showCode ? 'Diagram' : 'Code'}
          </Button>
        ) : null}
        <CopyCodeButton getText={() => code} />
      </div>
    </div>
  )
}
