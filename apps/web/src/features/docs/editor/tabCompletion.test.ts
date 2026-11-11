import { expect, test } from 'bun:test'
import { completeOnTab } from './tabCompletion'

function editable(open: boolean) {
  const element = document.createElement('div')
  if (open) {
    element.setAttribute('aria-expanded', 'true')
    element.setAttribute('aria-controls', 'bn-suggestion-menu')
  }
  const keys: string[] = []
  element.addEventListener('keydown', (event) => keys.push(event.key), true)
  element.addEventListener('keydown', completeOnTab(element), true)
  return { element, keys }
}

const tab = (init: KeyboardEventInit = {}) => new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true, ...init })

test('Tab picks the highlighted entry of an open menu (as Enter) and does not indent or move focus', () => {
  const { element, keys } = editable(true)
  const event = tab()
  element.dispatchEvent(event)
  expect(event.defaultPrevented).toBe(true)
  expect(keys).toEqual(['Tab', 'Enter'])
})

test('Tab without an open menu, and Shift+Tab, keep their normal meaning', () => {
  const closed = editable(false)
  const plain = tab()
  closed.element.dispatchEvent(plain)
  expect(plain.defaultPrevented).toBe(false)
  expect(closed.keys).toEqual(['Tab'])

  const open = editable(true)
  const shifted = tab({ shiftKey: true })
  open.element.dispatchEvent(shifted)
  expect(shifted.defaultPrevented).toBe(false)
  expect(open.keys).toEqual(['Tab'])
})
