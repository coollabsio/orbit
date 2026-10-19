import { expect, test } from 'bun:test'
import { isEmptyRichText } from './richText'

const text = (value: string) => ({ type: 'text', text: value, styles: {} })

test('empty paragraphs and blank text are empty', () => {
  expect(isEmptyRichText([])).toBe(true)
  expect(isEmptyRichText([{ type: 'paragraph', content: [] }])).toBe(true)
  expect(isEmptyRichText([{ type: 'paragraph', content: [text('  \n ')] }])).toBe(true)
})

test('text, a mention or an uploaded file is content', () => {
  expect(isEmptyRichText([{ type: 'paragraph', content: [text('Hi')] }])).toBe(false)
  expect(isEmptyRichText([{ type: 'paragraph', content: [{ type: 'mention', props: { userId: 'u', name: 'Ann' } }] }])).toBe(false)
  expect(isEmptyRichText([{ type: 'image', props: { url: '/api/x' } }])).toBe(false)
  expect(isEmptyRichText([{ type: 'paragraph', content: [{ type: 'link', href: 'https://x.dev', content: [text('x')] }] }])).toBe(false)
})

test('a file block still waiting for its upload is empty', () => {
  expect(isEmptyRichText([{ type: 'image', props: { url: '' } }])).toBe(true)
})

test('text nested in children and table cells counts', () => {
  expect(isEmptyRichText([{ type: 'paragraph', content: [], children: [{ type: 'paragraph', content: [text('child')] }] }])).toBe(false)
  expect(isEmptyRichText([{ type: 'table', content: { type: 'tableContent', rows: [{ cells: [{ type: 'tableCell', content: [text('cell')] }] }] } }])).toBe(false)
})
