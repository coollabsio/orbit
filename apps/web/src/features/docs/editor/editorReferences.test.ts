import { expect, test } from 'bun:test'
import { editorReference } from './editorReferences'

const ORIGIN = 'https://orbit.example'

function element(html: string) {
  const host = document.createElement('div')
  host.innerHTML = html
  return host.firstElementChild!
}

test('a mention chip names its member, a link to a task of this app names the task', () => {
  expect(editorReference(element('<span data-mention-user="u1">@Ann</span>'), ORIGIN)).toBe('user:u1')
  expect(editorReference(element(`<a href="${ORIGIN}/tasks/ENG-12">ENG-12 Ship</a>`), ORIGIN)).toBe('task:ENG-12')
  expect(editorReference(element(`<a href="${ORIGIN}/views/v1/ENG-3">x</a>`), ORIGIN)).toBe('task:ENG-3')
})

test('other links and plain text have no preview', () => {
  expect(editorReference(element('<a href="https://elsewhere.example/tasks/ENG-12">x</a>'), ORIGIN)).toBeNull()
  expect(editorReference(element(`<a href="${ORIGIN}/docs/0199a0b0-0000-7000-8000-000000000001">x</a>`), ORIGIN)).toBeNull()
  expect(editorReference(element('<span>ENG-12</span>'), ORIGIN)).toBeNull()
})
