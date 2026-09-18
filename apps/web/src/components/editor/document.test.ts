import { expect, test } from 'bun:test'
import {
  EMPTY_DOCUMENT,
  IDENTIFIER_PATTERN,
  asDocument,
  documentText,
  isEmptyDocument,
  taskIdentifiersInDocument,
} from './document'

const doc = {
  type: 'doc',
  content: [
    { type: 'heading', attrs: { level: 2 }, content: [{ type: 'text', text: 'Plan' }] },
    {
      type: 'bulletList',
      content: [
        {
          type: 'listItem',
          content: [{ type: 'paragraph', content: [{ type: 'text', text: 'outer' }] }],
        },
      ],
    },
    {
      type: 'paragraph',
      content: [
        { type: 'text', text: 'ping ' },
        { type: 'mention', attrs: { id: 'user-1', label: 'Ada Lovelace' } },
        { type: 'text', text: ' about ' },
        { type: 'taskMention', attrs: { id: 'task-1', identifier: 'ORB-12' } },
      ],
    },
  ],
}

test('documentText mirrors the server derivation across blocks and mentions', () => {
  expect(documentText(doc)).toBe('Plan\nouter\nping @Ada Lovelace about ORB-12')
})

test('empty documents are recognised without throwing on junk', () => {
  expect(isEmptyDocument(EMPTY_DOCUMENT)).toBe(true)
  expect(isEmptyDocument({ type: 'doc', content: [{ type: 'paragraph' }] })).toBe(true)
  expect(isEmptyDocument(doc)).toBe(false)
  expect(isEmptyDocument(null)).toBe(true)
  expect(isEmptyDocument('not a doc')).toBe(true)
})

test('asDocument coerces anything unusable to an empty document', () => {
  expect(asDocument(undefined)).toEqual(EMPTY_DOCUMENT)
  expect(asDocument({ type: 'paragraph' })).toEqual(EMPTY_DOCUMENT)
  expect(asDocument(doc)).toBe(doc as never)
})

test('taskIdentifiersInDocument collects chip identifiers once, in order', () => {
  const twice = {
    type: 'doc',
    content: [
      {
        type: 'paragraph',
        content: [
          { type: 'taskMention', attrs: { id: 'a', identifier: 'ORB-12' } },
          { type: 'taskMention', attrs: { id: 'b', identifier: 'ACME-3' } },
          { type: 'taskMention', attrs: { id: 'a', identifier: 'ORB-12' } },
        ],
      },
    ],
  }

  expect(taskIdentifiersInDocument(twice)).toEqual(['ORB-12', 'ACME-3'])
  expect(taskIdentifiersInDocument(EMPTY_DOCUMENT)).toEqual([])
})

test('IDENTIFIER_PATTERN matches project keys and numbers, not arbitrary words', () => {
  expect('ORB-12'.match(IDENTIFIER_PATTERN)?.[0]).toBe('ORB-12')
  expect('see ACME_2-134 now'.match(IDENTIFIER_PATTERN)?.[0]).toBe('ACME_2-134')
  expect('lowercase-12'.match(IDENTIFIER_PATTERN)).toBeNull()
  expect('ORB-'.match(IDENTIFIER_PATTERN)).toBeNull()
  expect('WORD-WORD'.match(IDENTIFIER_PATTERN)).toBeNull()
})
