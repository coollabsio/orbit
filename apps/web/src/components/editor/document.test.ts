import { expect, test } from 'bun:test'
import {
  EMPTY_DOCUMENT,
  IDENTIFIER_PATTERN,
  asDocument,
  documentText,
  editableDocument,
  isEmptyDocument,
  sameDocument,
  taskIdentifiersInDocument,
  toServerDocument,
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

test('toServerDocument strips the attributes TipTap emits that the server rejects', () => {
  const fromEditor = {
    type: 'doc',
    content: [
      { type: 'orderedList', attrs: { start: 1, type: null }, content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'x', marks: [{ type: 'link', attrs: { href: 'https://a.test', target: '_blank', rel: 'noopener noreferrer nofollow', class: null, title: null } }] }] }] }] },
      { type: 'orderedList', attrs: { start: 3, type: null }, content: [{ type: 'listItem', content: [{ type: 'paragraph' }] }] },
      { type: 'paragraph', content: [{ type: 'mention', attrs: { id: 'u1', label: 'Ada', mentionSuggestionChar: '@' } }] },
      { type: 'codeBlock', attrs: { language: null }, content: [{ type: 'text', text: 'fn' }] },
      { type: 'heading', attrs: { level: 5 }, content: [{ type: 'text', text: 'h' }] },
      { type: 'taskList', content: [{ type: 'taskItem', attrs: { checked: false }, content: [{ type: 'paragraph' }] }] },
      { type: 'paragraph' },
      { type: 'paragraph' },
    ],
  }

  expect(toServerDocument(fromEditor)).toEqual({
    type: 'doc',
    content: [
      { type: 'orderedList', content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'x', marks: [{ type: 'link', attrs: { href: 'https://a.test' } }] }] }] }] },
      { type: 'orderedList', attrs: { start: 3 }, content: [{ type: 'listItem', content: [{ type: 'paragraph' }] }] },
      { type: 'paragraph', content: [{ type: 'mention', attrs: { id: 'u1', label: 'Ada' } }] },
      { type: 'codeBlock', content: [{ type: 'text', text: 'fn' }] },
      { type: 'heading', attrs: { level: 3 }, content: [{ type: 'text', text: 'h' }] },
      { type: 'taskList', content: [{ type: 'taskItem', attrs: { checked: false }, content: [{ type: 'paragraph' }] }] },
    ],
  })
})

test('toServerDocument drops what cannot be made valid instead of sending it', () => {
  const junk = {
    type: 'doc',
    content: [
      { type: 'image', attrs: { src: 'x' } },
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'mail', marks: [{ type: 'link', attrs: { href: 'mailto:a@b.c' } }, { type: 'highlight' }] },
          { type: 'mention', attrs: { label: 'no id' } },
          { type: 'text', text: '' },
        ],
      },
    ],
  }

  expect(toServerDocument(junk)).toEqual({
    type: 'doc',
    content: [{ type: 'paragraph', content: [{ type: 'text', text: 'mail' }] }],
  })
  expect(toServerDocument(null)).toEqual({ type: 'doc', content: [] })
})

test('an editor round trip with only a trailing blank line is the same document', () => {
  const stored = { type: 'doc', content: [{ type: 'bulletList', content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'a' }] }] }] }] }
  const edited = { ...stored, content: [...stored.content, { type: 'paragraph' }] }

  expect(sameDocument(stored, edited)).toBe(true)
  expect(sameDocument(stored, EMPTY_DOCUMENT)).toBe(false)
})

test('an editor always gets at least one block to put the caret in', () => {
  expect(editableDocument(EMPTY_DOCUMENT)).toEqual({ type: 'doc', content: [{ type: 'paragraph' }] })
  expect(editableDocument(doc)).toEqual(toServerDocument(doc))
})
