import { describe, expect, test } from 'bun:test'
import { CODE_LANGUAGES, codeLines, detectLanguage, languageId } from './codeLanguages'

const text = (lines: ReturnType<typeof codeLines>) => lines.map((line) => line.map((token) => token.value).join('')).join('\n')
const types = (lines: ReturnType<typeof codeLines>, value: string) => lines.flat().find((token) => token.value === value)?.type

describe('languageId', () => {
  test('maps fence names and aliases to one id; an unknown name is plain text', () => {
    expect(languageId('js')).toBe('javascript')
    expect(languageId('TSX')).toBe('typescript')
    expect(languageId(' bash ')).toBe('shell')
    expect(languageId('rs')).toBe('rust')
    expect(languageId('Mermaid')).toBe('mermaid')
    expect(languageId('')).toBe('text')
    expect(languageId('plaintext')).toBe('text')
    expect(languageId('brainfuck')).toBe('text')
  })

  test('every language of the selector is its own id', () => {
    for (const language of CODE_LANGUAGES) expect(languageId(language.value)).toBe(language.value)
  })
})

describe('detectLanguage', () => {
  test('code that starts a Mermaid diagram is Mermaid', () => {
    expect(detectLanguage('graph TD\n    %% Nodes\n    A[Notion] --> C(Orbit)')).toBe('mermaid')
    expect(detectLanguage('\n%% a comment\nflowchart LR;\n A-->B')).toBe('mermaid')
    expect(detectLanguage('sequenceDiagram\n  Alice->>Bob: Hello')).toBe('mermaid')
    expect(detectLanguage('pie title Pets\n  "Dogs" : 3')).toBe('mermaid')
  })

  test('other code is plain text', () => {
    expect(detectLanguage('')).toBe('text')
    expect(detectLanguage('graph the results\nand more')).toBe('text')
    expect(detectLanguage('timeline of the project')).toBe('text')
    expect(detectLanguage('const pie = 1')).toBe('text')
    expect(detectLanguage('timeline')).toBe('text')
    expect(detectLanguage('graph TD\n%% only a comment')).toBe('text')
    expect(detectLanguage('fn main() {}\ngraph TD')).toBe('text')
  })
})

describe('codeLines', () => {
  test('the tokens join back to the code, in every language', () => {
    const code = 'fn main() {\n\tlet s = "a // b"; // note\n\n  /* x */ return 1.5 + \'c\';\n}\n'
    for (const language of CODE_LANGUAGES) expect(text(codeLines(code, language.value))).toBe(code)
  })

  test('finds keywords, strings and comments', () => {
    const lines = codeLines('const a = "x" // note', 'js')
    expect(types(lines, 'const')).toBe('keyword')
    expect(types(lines, '"x"')).toBe('string')
    expect(types(lines, '// note')).toBe('comment')
  })

  test('an emoji is one piece of a token, not two halves', () => {
    for (const language of ['yaml', 'shell', 'js']) {
      const values = codeLines('name: 🚀 Deploy', language).flat().map((token) => token.value)
      expect(values.join('')).toBe('name: 🚀 Deploy')
      expect(values.some((value) => value.includes('🚀'))).toBe(true)
    }
  })

  test('Rust has line and block comments', () => {
    expect(types(codeLines('let x = 1; // note', 'rust'), '// note')).toBe('comment')
    expect(types(codeLines('let x /* note */ = 1;', 'rust'), '/* note */')).toBe('comment')
  })

  test('plain text and Mermaid have no tokens', () => {
    expect(codeLines('const a = 1\nb', 'text')).toEqual([[{ type: 'identifier', value: 'const a = 1' }], [{ type: 'identifier', value: 'b' }]])
    expect(codeLines('graph TD', 'mermaid')).toEqual([[{ type: 'identifier', value: 'graph TD' }]])
  })
})
