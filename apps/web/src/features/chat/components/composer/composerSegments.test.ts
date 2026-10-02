import { expect, test } from 'bun:test'
import { composerSegments } from './composerSegments'

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
    { kind: 'text', text: '@Bob a@Ann `@Ann` ' },
    { kind: 'mention', text: '@here' },
  ])
})
