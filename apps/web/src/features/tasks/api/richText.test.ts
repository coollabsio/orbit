import { expect, test } from 'bun:test'
import { documentFromText, emptyDocument } from './richText'

test('plain text becomes one paragraph per line', () => {
  expect(documentFromText('first\n\nsecond')).toEqual({
    type: 'doc',
    content: [
      { type: 'paragraph', content: [{ type: 'text', text: 'first' }] },
      { type: 'paragraph', content: [{ type: 'text', text: 'second' }] },
    ],
  })
  expect(documentFromText('')).toEqual(emptyDocument())
  expect(documentFromText('   ').type).toBe('doc')
})

test('a picked mention becomes a real mention node so the server notifies', () => {
  const document = documentFromText('Hey @Ada thanks', [{ id: 'user-2', label: 'Ada' }])

  expect(document).toEqual({
    type: 'doc',
    content: [
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'Hey ' },
          { type: 'mention', attrs: { id: 'user-2', label: 'Ada' } },
          { type: 'text', text: ' thanks' },
        ],
      },
    ],
  })
})

test('the longest label wins so a prefix name does not steal the match', () => {
  const document = documentFromText('ping @Ada Lovelace', [
    { id: 'short', label: 'Ada' },
    { id: 'long', label: 'Ada Lovelace' },
  ])
  const content = (document.content as { content: { attrs?: { id: string } }[] }[])[0].content

  expect(content[1].attrs?.id).toBe('long')
})
