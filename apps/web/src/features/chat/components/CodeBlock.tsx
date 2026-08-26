// the chat reference CodeBlock + highlightCode (MessageItem.tsx): fenced code with copy button and
// hand-rolled JS/Rust token colors.
import { useState } from 'react'
import { Copy } from 'reicon-react'

/* ---------- code block (the chat reference CodeBlock + highlightCode) ---------- */

function highlightCode(code: string, language: string): React.ReactNode[] {
  const normalizedLanguage = language.toLowerCase()
  const keywords =
    normalizedLanguage === 'rust'
      ? 'as|async|await|break|const|continue|crate|dyn|else|enum|extern|fn|for|if|impl|in|let|loop|match|mod|move|mut|pub|ref|return|self|Self|static|struct|super|trait|type|unsafe|use|where|while'
      : 'break|case|catch|class|const|continue|default|else|export|for|function|if|import|let|new|return|switch|throw|try|var|while'
  const types =
    normalizedLanguage === 'rust'
      ? 'bool|char|f32|f64|i8|i16|i32|i64|i128|isize|str|String|u8|u16|u32|u64|u128|usize|Option|Result|Vec'
      : 'boolean|number|string|Array|Promise|Record|Set|Map'
  const tokenRegex = new RegExp(
    `(//.*|#.*|"(?:\\\\.|[^"\\\\])*"|'(?:\\\\.|[^'\\\\])*'|\\b(?:${keywords})\\b|\\b(?:${types})\\b|\\b\\d+(?:\\.\\d+)?\\b)`,
    'g',
  )
  const typeRegex = new RegExp(`^(?:${types})$`)
  const parts: React.ReactNode[] = []
  let lastIndex = 0
  let tokenIndex = 0
  let match: RegExpExecArray | null

  while ((match = tokenRegex.exec(code)) !== null) {
    if (match.index > lastIndex) parts.push(code.slice(lastIndex, match.index))
    const token = match[0]
    let kind = 'keyword'
    if (token.startsWith('//') || token.startsWith('#')) kind = 'comment'
    else if (token.startsWith('"') || token.startsWith("'")) kind = 'string'
    else if (/^\d/.test(token)) kind = 'number'
    else if (typeRegex.test(token)) kind = 'type'
    parts.push(
      <span key={`code-token-${match.index}-${tokenIndex}`} className="fc-tok" data-kind={kind}>
        {token}
      </span>,
    )
    lastIndex = match.index + token.length
    tokenIndex += 1
  }

  if (lastIndex < code.length) parts.push(code.slice(lastIndex))
  return parts.length > 0 ? parts : [code]
}

function copyTextFallback(text: string) {
  const textarea = document.createElement('textarea')
  textarea.value = text
  textarea.setAttribute('readonly', '')
  textarea.style.position = 'fixed'
  textarea.style.top = '-9999px'
  textarea.style.left = '-9999px'
  document.body.appendChild(textarea)
  textarea.select()
  textarea.setSelectionRange(0, textarea.value.length)
  const copied = document.execCommand('copy')
  document.body.removeChild(textarea)
  if (!copied) throw new Error('Copy failed')
}

export function CodeBlock({ code, language = '' }: { code: string; language?: string }) {
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'failed'>('idle')

  async function copyCode() {
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(code)
      else copyTextFallback(code)
      setCopyState('copied')
    } catch {
      try {
        copyTextFallback(code)
        setCopyState('copied')
      } catch {
        setCopyState('failed')
      }
    }
    setTimeout(() => setCopyState('idle'), 1600)
  }

  return (
    <div className="fc-codeblock">
      <pre>
        <code>{highlightCode(code, language)}</code>
      </pre>
      <button type="button" className="fc-codeblock-copy" data-state={copyState} title="Copy code" onClick={copyCode}>
        <Copy size={14} />
        {copyState === 'copied' ? 'Copied' : copyState === 'failed' ? 'Failed' : 'Copy'}
      </button>
    </div>
  )
}

