// The languages of a code block (tasks, comments, chat, docs) and their syntax tokens. `sugar-high` is the tokenizer:
// it is small and gives plain tokens, so each surface draws them with its own elements.
import { parse, type ParseOptions, type TokenType } from 'sugar-high/core'
import { lang, languages } from 'sugar-high/lang'

export type { TokenType }

/** A block with this language shows its diagram. */
export const MERMAID_LANGUAGE = 'mermaid'
export const PLAIN_LANGUAGE = 'text'

const NAMES: Record<string, string> = {
  javascript: 'JavaScript',
  typescript: 'TypeScript',
  css: 'CSS',
  python: 'Python',
  c: 'C',
  go: 'Go',
  java: 'Java',
  rust: 'Rust',
  json: 'JSON',
  diff: 'Diff',
  shell: 'Shell',
  cpp: 'C++',
  csharp: 'C#',
  sql: 'SQL',
  html: 'HTML',
  vue: 'Vue',
  svelte: 'Svelte',
  yaml: 'YAML',
  markdown: 'Markdown',
  ruby: 'Ruby',
  kotlin: 'Kotlin',
  swift: 'Swift',
  php: 'PHP',
  toml: 'TOML',
  powershell: 'PowerShell',
  dockerfile: 'Dockerfile',
  graphql: 'GraphQL',
  hcl: 'HCL',
  zig: 'Zig',
  lua: 'Lua',
}

const configs = new Map<string, ParseOptions | undefined>(languages.map((language) => [language.id, language.config]))
// The Rust preset has no comments; they are those of C.
configs.set('rust', { ...configs.get('rust'), onCommentStart: configs.get('c')?.onCommentStart, onCommentEnd: configs.get('c')?.onCommentEnd })

/** The languages a person can select, plain text and Mermaid first. */
export const CODE_LANGUAGES: readonly { value: string; label: string }[] = [
  { value: PLAIN_LANGUAGE, label: 'Plain text' },
  { value: MERMAID_LANGUAGE, label: 'Mermaid' },
  ...Object.entries(NAMES)
    .map(([value, label]) => ({ value, label }))
    .sort((a, b) => a.label.localeCompare(b.label)),
]

/** The language of a name as a person writes it after a code fence (`js`, `TSX`, `bash`); an unknown name is plain text. */
export function languageId(name: string): string {
  const normalized = name.trim().toLowerCase()
  if (normalized === MERMAID_LANGUAGE) return MERMAID_LANGUAGE
  const id = lang(normalized)
  return id && id in NAMES ? id : PLAIN_LANGUAGE
}

const MERMAID_START =
  /^(?:(?:graph|flowchart)\s+(?:TB|TD|BT|RL|LR)\s*;?|sequenceDiagram|classDiagram(?:-v2)?|stateDiagram(?:-v2)?|erDiagram|gitGraph|mindmap|gantt|journey|timeline|quadrantChart|requirementDiagram|pie(?:\s+(?:showData|title\b.*))?)$/

/**
 * The language of a code block that has a fence without a name. People paste a diagram without the name `mermaid`, so
 * code whose first line starts a Mermaid diagram (`graph TD`, `sequenceDiagram`) and that has more lines is one. A line of `%%` is a comment.
 */
export function detectLanguage(code: string): string {
  const lines = code.split('\n').filter((line) => line.trim() && !line.trim().startsWith('%%'))
  // A diagram has a body: one line (`timeline`) is a word.
  return lines.length > 1 && MERMAID_START.test(lines[0]!.trim()) ? MERMAID_LANGUAGE : PLAIN_LANGUAGE
}

export interface CodeToken {
  type: TokenType
  value: string
}

/** The tokens of `code`, line by line. The values of a line join back to its text; plain text has no tokens. */
export function codeLines(code: string, language: string): CodeToken[][] {
  const id = languageId(language)
  if (!(id in NAMES)) return code.split('\n').map((line) => [{ type: 'identifier', value: line }])
  return parse(code, configs.get(id)).lines.map((line) => {
    const tokens: CodeToken[] = []
    for (const token of line.tokens) {
      const last = tokens.at(-1)
      // One token for a run of one type: the tokenizer cuts an emoji into its two halves, and a half is not a character.
      if (last?.type === token.type) last.value += token.value
      else tokens.push({ type: token.type, value: token.value })
    }
    return tokens
  })
}
