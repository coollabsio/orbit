import { expect, test } from 'bun:test'
import { BlockNoteEditor } from '@blocknote/core'
import { buildMentionTokens } from '@/lib/mentions'
import { blocksToMarkdown, markdownToBlocks, normalizeMarkdown, roundTrips, toggleChecklistLine } from './convert'
import { markdownEditorSchema } from './schema'

const editor = () => BlockNoteEditor.create({ schema: markdownEditorSchema, _tiptapOptions: { injectCSS: false } })
const tokens = buildMentionTokens([
  { id: 'u1', name: 'Ada Lovelace', handle: 'ada' },
  { id: 'u2', name: 'Grace', handle: 'grace' },
])

// The corpus of step 0: what people write in task descriptions and comments. Each sample comes back unchanged.
const SURVIVES: Record<string, string> = {
  paragraph: 'Plain text with **bold**, *italic*, ~~strike~~ and `code`.',
  paragraphs: 'First paragraph.\n\nSecond paragraph.',
  lineBreak: 'Line one\nline two',
  headings: '# One\n\n## Two\n\n### Three',
  bullets: '- one\n- two\n  - nested\n- three',
  numbered: '1. one\n2. two\n3. three',
  checklist: '- [ ] open\n- [x] done\n  - [ ] nested',
  nestedMixed: '1. first\n   - inner bullet\n   - [ ] inner check\n2. second',
  quote: '> quoted text\n> second line',
  code: '```ts\nconst a = 1\n  indented()\n```',
  codeNoLanguage: '```\nplain * text\n```',
  mermaid: '```mermaid\ngraph TD\n  A --> B\n```',
  table: '| Name | Value |\n| --- | --- |\n| a | 1 |\n| b | 2 |',
  externalImage: '![diagram](https://example.com/diagram.png)',
  attachmentImage: 'Before\n\n![shot.png](/api/v1/workspaces/0190f5b4-7c1e-7a3b-8c4d-5e6f7a8b9c0d/tasks/0190f5b4-7c1e-7a3b-8c4d-5e6f7a8b9c0e/attachments/0190f5b4-7c1e-7a3b-8c4d-5e6f7a8b9c0f/download)',
  divider: 'above\n\n---\n\nbelow',
  link: 'See [the docs](https://example.com/docs) for more.',
  bareUrl: 'See https://github.com/coollabsio/orbit/pull/12 for the fix.',
  taskUrl: 'See https://orbit.test/tasks/ENG-12?x=1&y=2 here.',
  mention: 'Thanks @Ada Lovelace and @grace, please look.',
  identifiers: 'Blocked by ENG-12 and relates to ORB-3.',
  githubReferences: 'Fixes coollabsio/orbit#12 and #34.',
  snakeCase: 'Call some_function_name() in snake_case_file.rs and 2 * 3 * 4',
  headingAndList: '## Steps\n\n1. Do this\n2. Do that\n\nDone.',
  inlineCode: 'Run `a | b` and `**not bold**`.',
  emoji: 'Ship it :rocket: 🚀 :custom_emoji:',
  symbols: 'Issue #12, 5 > 3, a -> b, cost is $5, path C:\\Users\\me',
}

// What the editor cannot hold. These must be found, so the caller edits them as plain markdown.
const LOSSY: Record<string, string> = {
  rawHtml: 'Use Vec<String> and <tag> here.',
  kbd: 'Press <kbd>Ctrl</kbd> now.',
  underline: 'This is __underlined__ text.',
  listInQuote: '> - item\n> - item two',
  codeInList: '- item\n\n  ```sh\n  ls -la\n  ```\n- next',
  escapedStars: 'a \\*literal\\* star',
  indentedCode: 'text\n\n    indented code',
}

test('the corpus survives markdown → blocks → markdown', () => {
  const instance = editor()
  for (const [name, source] of Object.entries(SURVIVES)) {
    expect([name, blocksToMarkdown(instance, markdownToBlocks(instance, source, tokens))]).toEqual([name, source])
    expect([name, roundTrips(instance, source, tokens)]).toEqual([name, true])
  }
})

test('text that the editor would change is found', () => {
  const instance = editor()
  for (const [name, source] of Object.entries(LOSSY)) expect([name, roundTrips(instance, source, tokens)]).toEqual([name, false])
})

test('other spellings of the same markdown are equal after normalisation', () => {
  expect(normalizeMarkdown('* one\n* [ ] two\n\n***\n\n| a   | b |\n| :-- | --: |\n| 1 | 2   |\r\n')).toBe('- one\n- [ ] two\n\n---\n\n| a | b |\n| :--- | ---: |\n| 1 | 2 |')
  expect(normalizeMarkdown('one  \ntwo\\\n three\n\n\n\nfour  ')).toBe('one\ntwo\nthree\n\nfour')
  // a code block is not touched, except the "text" language that BlockNote adds
  expect(normalizeMarkdown('```text\n* not a bullet  \n***\n```')).toBe('```\n* not a bullet  \n***\n```')
  expect(normalizeMarkdown('```python\n| a   | b |\n```')).toBe('```python\n| a   | b |\n```')
  // blank lines are content in a code block, and one blank line at most outside it
  expect(normalizeMarkdown('one\n\n\n\n```\na\n\n\n\nb\n```\n\n\n\ntwo')).toBe('one\n\n```\na\n\n\n\nb\n```\n\ntwo')
  const instance = editor()
  for (const source of ['* one\n* two', 'above\n\n***\n\nbelow', 'Line one  \nline two', '| a   | b   |\n| --- | --- |\n| 1   | 2   |']) {
    expect([source, roundTrips(instance, source)]).toEqual([source, true])
  }
})

test('a mention is a chip in the editor and @Name in the markdown', () => {
  const instance = editor()
  const blocks = markdownToBlocks(instance, 'Ask @ada or **@Grace**. Not a@grace, @nobody, or `@ada`.', tokens)
  const inline = JSON.stringify(blocks)
  // styled text stays text: a chip has no styles to keep the bold
  expect(inline.match(/"type":"mention"/g)).toHaveLength(1)
  expect(inline).toContain('"props":{"userId":"u1","name":"ada"}')
  expect(blocksToMarkdown(instance, blocks)).toBe('Ask @ada or **@Grace**. Not a@grace, @nobody, or `@ada`.')
  // a chip that the picker made
  expect(blocksToMarkdown(instance, [{ type: 'paragraph', content: ['Hi ', { type: 'mention', props: { userId: 'u1', name: 'Ada Lovelace' } }, '!'] }])).toBe('Hi @Ada Lovelace!')
})

test('a checklist tick changes only its own line', () => {
  const source = '- [ ] one\n  - [x] nested\n```\n- [ ] in code\n```\n1. [ ] numbered\n- [ ] last\ntext [ ] here'
  expect(toggleChecklistLine(source, 0, true)).toBe(source.replace('- [ ] one', '- [x] one'))
  expect(toggleChecklistLine(source, 1, false)).toBe(source.replace('- [x] nested', '- [ ] nested'))
  expect(toggleChecklistLine(source, 6, true)).toBe(source.replace('- [ ] last', '- [x] last'))
  // not a checklist item for the renderer: plain text, a numbered item, a line out of range
  expect(toggleChecklistLine(source, 7, true)).toBeNull()
  expect(toggleChecklistLine(source, 5, true)).toBeNull()
  expect(toggleChecklistLine(source, 99, true)).toBeNull()
  expect(toggleChecklistLine('- [ ]', 0, true)).toBe('- [x]')
  expect(toggleChecklistLine('a\r\n- [X] b', 1, false)).toBe('a\n- [ ] b')
})
