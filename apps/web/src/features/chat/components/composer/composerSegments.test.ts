import { describe, expect, test } from 'bun:test'
import { completeCodeFence, composerSegments, inOpenCodeBlock } from './composerSegments'

const people = [
  { id: 'u1', name: 'Ann' },
  { id: 'u2', name: 'Ann Lee' },
]
const channels = [{ id: 'c1', name: 'design' }]

test('mentions and emoji are cut out of the text, and the pieces join back to it', () => {
  const text = 'Hi @Ann Lee, see #design 🎉 now'
  const segments = composerSegments(text, people, channels)
  expect(segments).toEqual([
    { kind: 'text', text: 'Hi ' },
    { kind: 'mention', text: '@Ann Lee' },
    { kind: 'text', text: ', see ' },
    { kind: 'mention', text: '#design' },
    { kind: 'text', text: ' ' },
    { kind: 'emoji', text: '🎉', url: '/assets/twemoji-17.0.3/1f389.svg' },
    { kind: 'text', text: ' now' },
  ])
  expect(segments.map((segment) => segment.text).join('')).toBe(text)
})

test('a name that nobody has, a mention inside a word and a mention in code stay text', () => {
  expect(composerSegments('@Bob a@Ann `@Ann` @here', people, channels)).toEqual([
    { kind: 'text', text: '@Bob a@Ann ' },
    { kind: 'text', text: '`', marks: ['code', 'marker'] },
    { kind: 'text', text: '@Ann', marks: ['code'] },
    { kind: 'text', text: '`', marks: ['code', 'marker'] },
    { kind: 'text', text: ' ' },
    { kind: 'mention', text: '@here' },
  ])
})

/** `[text, marks]` of each piece, the marks joined: short enough to read a whole line of markdown at once. */
function marked(text: string) {
  return composerSegments(text, people, channels).map((segment) => [segment.text, segment.marks?.join(' ') ?? ''])
}

test('the pieces of a text with every syntax join back to it', () => {
  const text = [
    '# Title **bold** ***both*** *it* _it_ __under__ ~~gone~~ `code` ||secret|| \\*not\\*',
    '## Two',
    '### Three @Ann 🎉',
    '-# small print',
    '> quoted **@Ann Lee** line',
    '```ts',
    'const a = **1** // @Ann',
    '``` after',
    'unclosed **bold and `code and ||spoiler',
    '>>> the rest',
    'of _the_ text',
    '```',
    'open fence',
  ].join('\n')
  expect(
    composerSegments(text, people, channels)
      .map((segment) => segment.text)
      .join(''),
  ).toBe(text)
})

test('each inline syntax marks its content, and its markers as markers', () => {
  expect(marked('a **b** c')).toEqual([
    ['a ', ''],
    ['**', 'strong marker'],
    ['b', 'strong'],
    ['**', 'strong marker'],
    [' c', ''],
  ])
  expect(marked('***b***')).toEqual([
    ['***', 'strong em marker'],
    ['b', 'strong em'],
    ['***', 'strong em marker'],
  ])
  expect(marked('*b* _c_')).toEqual([
    ['*', 'em marker'],
    ['b', 'em'],
    ['*', 'em marker'],
    [' ', ''],
    ['_', 'em marker'],
    ['c', 'em'],
    ['_', 'em marker'],
  ])
  expect(marked('__b__')).toEqual([
    ['__', 'underline marker'],
    ['b', 'underline'],
    ['__', 'underline marker'],
  ])
  expect(marked('~~b~~')).toEqual([
    ['~~', 'strike marker'],
    ['b', 'strike'],
    ['~~', 'strike marker'],
  ])
  expect(marked('`b`')).toEqual([
    ['`', 'code marker'],
    ['b', 'code'],
    ['`', 'code marker'],
  ])
  expect(marked('||b||')).toEqual([
    ['||', 'spoiler marker'],
    ['b', 'spoiler'],
    ['||', 'spoiler marker'],
  ])
})

test('formats nest, and a longer run of markers closes the inner format first', () => {
  expect(marked('**a ~~b~~ *c***')).toEqual([
    ['**', 'strong marker'],
    ['a ', 'strong'],
    ['~~', 'strong strike marker'],
    ['b', 'strong strike'],
    ['~~', 'strong strike marker'],
    [' ', 'strong'],
    ['*', 'strong em marker'],
    ['c', 'strong em'],
    ['*', 'strong em marker'],
    ['**', 'strong marker'],
  ])
  expect(marked('*a **b** c*').map(([, marks]) => marks)).toEqual(['em marker', 'em', 'em strong marker', 'em strong', 'em strong marker', 'em', 'em marker'])
})

test('syntax that is not closed, arithmetic and a name with underscores stay plain text', () => {
  for (const text of ['**abc', 'abc**', '*abc', '__abc', '~~abc', '`abc', '||abc', '2 * 3 * 4', 'snake_case_name', '**a\nb**', '~ a ~ | b |']) {
    expect(marked(text)).toEqual([[text, '']])
  }
})

test('nothing is parsed inside inline code', () => {
  expect(marked('`**a** _b_ \\* ||c||` **d**')).toEqual([
    ['`', 'code marker'],
    ['**a** _b_ \\* ||c||', 'code'],
    ['`', 'code marker'],
    [' ', ''],
    ['**', 'strong marker'],
    ['d', 'strong'],
    ['**', 'strong marker'],
  ])
  // Code closes before bold does, so these stars are text.
  expect(marked('**a `b** c`')).toEqual([
    ['**a ', ''],
    ['`', 'code marker'],
    ['b** c', 'code'],
    ['`', 'code marker'],
  ])
})

test('a fence is code from its opening line to its closing line, and to the end of the text when it is not closed', () => {
  expect(marked('a\n```ts\n**b** `c`\n> d\n```\n**e**')).toEqual([
    ['a\n', ''],
    ['```ts', 'code marker'],
    ['\n**b** `c`\n> d\n', 'code'],
    ['```', 'code marker'],
    ['\n', ''],
    ['**', 'strong marker'],
    ['e', 'strong'],
    ['**', 'strong marker'],
  ])
  expect(marked('```\n**b**\n# c')).toEqual([
    ['```', 'code marker'],
    ['\n**b**\n# c', 'code'],
  ])
  expect(marked('```a b``` *c*')).toEqual([
    ['```', 'code marker'],
    ['a b', 'code'],
    ['```', 'code marker'],
    [' ', ''],
    ['*', 'em marker'],
    ['c', 'em'],
    ['*', 'em marker'],
  ])
})

test('a backslash takes the meaning from the punctuation after it', () => {
  expect(marked('\\*a* \\**b**')).toEqual([
    ['\\', 'marker'],
    ['*a* ', ''],
    ['\\', 'marker'],
    ['**b**', ''],
  ])
  expect(marked('**a \\** b**')).toEqual([
    ['**', 'strong marker'],
    ['a ', 'strong'],
    ['\\', 'strong marker'],
    ['** b', 'strong'],
    ['**', 'strong marker'],
  ])
  expect(marked('a\\b')).toEqual([['a\\b', '']])
})

test('a mention and an emoji inside a format keep their kind and get its marks', () => {
  expect(composerSegments('**hi @Ann Lee 🎉**', people, channels)).toEqual([
    { kind: 'text', text: '**', marks: ['strong', 'marker'] },
    { kind: 'text', text: 'hi ', marks: ['strong'] },
    { kind: 'mention', text: '@Ann Lee', marks: ['strong'] },
    { kind: 'text', text: ' ', marks: ['strong'] },
    { kind: 'emoji', text: '🎉', url: '/assets/twemoji-17.0.3/1f389.svg', marks: ['strong'] },
    { kind: 'text', text: '**', marks: ['strong', 'marker'] },
  ])
})

test('a marker character in the name of a mention starts nothing', () => {
  const named = [{ id: 'c2', name: 'dev_ops' }]
  expect(composerSegments('_a #dev_ops b', people, named)).toEqual([
    { kind: 'text', text: '_a ' },
    { kind: 'mention', text: '#dev_ops' },
    { kind: 'text', text: ' b' },
  ])
})

test('line prefixes are markers: quote, heading and subtext', () => {
  expect(marked('> a *b*\nc')).toEqual([
    ['> ', 'marker'],
    ['a ', 'muted'],
    ['*', 'muted em marker'],
    ['b', 'muted em'],
    ['*', 'muted em marker'],
    ['\n', 'muted'],
    ['c', ''],
  ])
  expect(marked('# a\n## b\n### c\n#### d\n#e')).toEqual([
    ['# ', 'marker'],
    ['a\n', ''],
    ['## ', 'marker'],
    ['b\n', ''],
    ['### ', 'marker'],
    ['c\n#### d\n#e', ''],
  ])
  expect(marked('-# a')).toEqual([
    ['-# ', 'muted marker'],
    ['a', 'muted'],
  ])
  expect(marked('a > b # c')).toEqual([['a > b # c', '']])
})

test('`>>>` quotes everything to the end of the text', () => {
  expect(marked('a\n>>> b\n> c\n## d')).toEqual([
    ['a\n', ''],
    ['>>> ', 'marker'],
    ['b\n> c\n', 'muted'],
    ['## ', 'muted marker'],
    ['d', 'muted'],
  ])
})

describe('inOpenCodeBlock', () => {
  test('is true between an opening fence and its closing fence', () => {
    const text = 'see\n```js\nconst a = 1\n```\nafter'
    expect(inOpenCodeBlock(text, 2)).toBe(false)
    expect(inOpenCodeBlock(text, text.indexOf('const'))).toBe(true)
    expect(inOpenCodeBlock(text, text.indexOf('after'))).toBe(false)
    expect(inOpenCodeBlock('```', 3)).toBe(true)
    expect(inOpenCodeBlock('```a``` b', 9)).toBe(false)
  })

  test('a fence in the middle of a line opens nothing', () => {
    expect(inOpenCodeBlock('what does ``` mean?', 19)).toBe(false)
    expect(inOpenCodeBlock('a ```\nb', 7)).toBe(false)
  })
})

describe('completeCodeFence', () => {
  test('an opening fence gets its closing fence on the next line; the caret stays after the opening one', () => {
    expect(completeCodeFence('```', 3)).toEqual({ text: '```\n```', cursor: 3 })
    expect(completeCodeFence('see\n```\nafter', 7)).toEqual({ text: 'see\n```\n```\nafter', cursor: 7 })
  })

  test('a typed closing fence replaces the one that was added', () => {
    const text = '```js\nconst a = 1\n```\n```'
    expect(completeCodeFence(text, text.length - 4)).toEqual({ text: '```js\nconst a = 1\n```', cursor: text.length - 4 })
  })

  test('no change for a closed block, text after the caret, two or four backticks', () => {
    expect(completeCodeFence('```a\n```', 8)).toBeNull()
    expect(completeCodeFence('```rest', 3)).toBeNull()
    expect(completeCodeFence('``', 2)).toBeNull()
    expect(completeCodeFence('````', 4)).toBeNull()
    expect(completeCodeFence('a ```', 5)).toBeNull()
  })
})
