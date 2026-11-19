import { expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { Window } from 'happy-dom'

// The message frame's script lives in the server's view document (crates/mail/src/html.rs). These tests run that very
// script on a mail body in a page of its own, as the frame does.
const source = readFileSync(new URL('../../../../../crates/mail/src/html.rs', import.meta.url), 'utf8')
const script = source.match(/const FRAME_SCRIPT: &str = r#"([\s\S]*?)"#;/)?.[1] ?? ''

function frame(body: string, htmlAttributes = '') {
  const window = new Window({ settings: { enableJavaScriptEvaluation: true, suppressInsecureJavaScriptEnvironmentWarning: true } })
  const posts: Array<Record<string, unknown>> = []
  // the frame is its own parent here: keep its posts instead of sending them to itself
  ;(window as unknown as { postMessage: (data: Record<string, unknown>) => void }).postMessage = (data) => posts.push(data)
  // happy-dom has no layout: a size is "seen" once, when the frame starts observing
  ;(window as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    callback: () => void
    constructor(callback: () => void) {
      this.callback = callback
    }
    observe() {
      this.callback()
    }
  }
  window.document.write(`<!DOCTYPE html><html${htmlAttributes}><head></head><body>${body}<script>${script}</script></body></html>`)
  const document = window.document
  return { document, posts, close: () => window.close() }
}

test('the script is found', () => {
  expect(script.length).toBeGreaterThan(500)
})

test('a trailing blockquote folds with its "wrote:" line, also inside a div', () => {
  for (const body of [
    '<p>Thanks!</p><p>On Mon, Ann wrote:</p><blockquote>old</blockquote>',
    '<div><p>Thanks!</p><p>On Mon, Ann wrote:</p><blockquote>old</blockquote></div>',
  ]) {
    const { document, close } = frame(body)
    const button = document.querySelector('button.orbit-fold')
    expect(button?.getAttribute('aria-expanded')).toBe('false')
    expect(document.querySelector('blockquote')?.hasAttribute('hidden')).toBe(true)
    expect([...document.querySelectorAll('p')].map((p) => p.hasAttribute('hidden'))).toEqual([false, true])
    ;(button as unknown as HTMLElement).click()
    expect(button?.getAttribute('aria-expanded')).toBe('true')
    expect(document.querySelector('blockquote')?.hasAttribute('hidden')).toBe(false)
    close()
  }
})

test('nothing folds when only the quote would stay', () => {
  const { document, close } = frame('<blockquote>old</blockquote>')
  expect(document.querySelector('button.orbit-fold')).toBeNull()
  close()
})

test('the blocked image count is the one the server wrote', () => {
  // a cid image with no such part has no source either, but loading images would not help it
  const { posts, close } = frame('<p>x</p><img alt="logo">', ' data-orbit-blocked="2"')
  expect(posts.at(-1)?.orbitMailBlocked).toBe(2)
  close()
  const none = frame('<p>x</p><img alt="logo">')
  expect(none.posts.at(-1)?.orbitMailBlocked).toBe(0)
  none.close()
})
