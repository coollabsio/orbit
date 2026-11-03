import { expect, test } from 'bun:test'

import { markdownToText } from './markdown'

test('keeps paragraphs and list structure, drops the markup', () => {
  const markdown = [
    '## Plan',
    '',
    'Ship **the importer** with _care_ and a [link](https://example.com).',
    '',
    '',
    '',
    '- first `snake_case` item',
    '  * nested ~~old~~',
    '- [ ] open',
    '- [x] done',
    '1. one',
    '> quoted *text*',
    '---',
    '```ts',
    'const **raw** = 1',
    '```',
    '![diagram](x.png) and file_name_here',
  ].join('\n')
  expect(markdownToText(markdown)).toBe(
    [
      'Plan',
      '',
      'Ship the importer with care and a link.',
      '',
      '• first snake_case item',
      '  • nested old',
      '☐ open',
      '☑ done',
      '1. one',
      'quoted text',
      '',
      'const **raw** = 1',
      'diagram and file_name_here',
    ].join('\n'),
  )
})

test('plain text passes through', () => {
  expect(markdownToText('A safe, idempotent development seed record.')).toBe('A safe, idempotent development seed record.')
})
