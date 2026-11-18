import { afterEach, expect, test } from 'bun:test'
import { writeClipboard } from './clipboard'

const originalClipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard')
const originalExec = document.execCommand

afterEach(() => {
  if (originalClipboard) Object.defineProperty(navigator, 'clipboard', originalClipboard)
  else Reflect.deleteProperty(navigator, 'clipboard')
  document.execCommand = originalExec
})

function setClipboard(value: unknown) {
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value })
}

/** Records what `execCommand('copy')` would copy: the selected text of the focused text area. */
function fakeExec(result: boolean) {
  const copied: string[] = []
  document.execCommand = ((command: string) => {
    const area = document.activeElement
    if (command === 'copy' && area instanceof HTMLTextAreaElement) copied.push(area.value.slice(area.selectionStart, area.selectionEnd))
    return result
  }) as typeof document.execCommand
  return copied
}

test('uses the Clipboard API when the page has it', async () => {
  const written: string[] = []
  setClipboard({ writeText: async (text: string) => { written.push(text) } })
  const copied = fakeExec(true)
  await writeClipboard('v=spf1 mx ~all')
  expect(written).toEqual(['v=spf1 mx ~all'])
  expect(copied).toEqual([])
})

test('copies through a text area on a page without the Clipboard API (plain HTTP)', async () => {
  setClipboard(undefined)
  const copied = fakeExec(true)
  const button = document.createElement('button')
  document.body.appendChild(button)
  button.focus()
  await writeClipboard('_orbit-verify.example.com')
  expect(copied).toEqual(['_orbit-verify.example.com'])
  expect(document.querySelector('textarea')).toBeNull()
  expect(document.activeElement).toBe(button)
  button.remove()
})

test('falls back when the Clipboard API refuses', async () => {
  setClipboard({ writeText: async () => { throw new DOMException('denied', 'NotAllowedError') } })
  const copied = fakeExec(true)
  await writeClipboard('token')
  expect(copied).toEqual(['token'])
})

test('fails when the browser refuses both ways', async () => {
  setClipboard(undefined)
  fakeExec(false)
  await expect(writeClipboard('token')).rejects.toThrow()
})
