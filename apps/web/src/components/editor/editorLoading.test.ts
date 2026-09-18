import { expect, test } from 'bun:test'

const source = (name: string) => Bun.file(new URL(`./${name}`, import.meta.url)).text()

test('RichTextEditor loads the ProseMirror surface lazily so list views stay light', async () => {
  const wrapper = await source('RichTextEditor.tsx')

  expect(wrapper).toContain('lazy(')
  expect(wrapper).toContain("import('./RichTextEditorSurface')")
  expect(wrapper).toContain('Suspense')
  // The wrapper itself must not pull TipTap into the caller's chunk; its only
  // other imports are erased types.
  expect(wrapper).not.toContain('@tiptap/')
  expect(wrapper).not.toMatch(/^import (?!type )(?!\{ Suspense, lazy \} from 'react').*$/m)
})

test('only the surface and its extension set import the TipTap editor', async () => {
  const surface = await source('RichTextEditorSurface.tsx')
  const extensions = await source('extensions/editorExtensions.ts')

  expect(surface).toContain("from '@tiptap/react'")
  expect(surface).toContain("from './extensions/editorExtensions'")
  expect(extensions).toContain("from '@tiptap/starter-kit'")
  expect(extensions).toContain("from './personMention'")
  expect(await source('extensions/personMention.ts')).toContain("from '@tiptap/extension-mention'")

  const view = await source('RichTextView.tsx')
  expect(view).toContain('@tiptap/static-renderer/json/react')
  expect(view).not.toContain("from '@tiptap/react'")
  expect(view).not.toContain('@tiptap/pm')
})

test('the picker and the chip modules stay free of TipTap so they can load eagerly', async () => {
  for (const name of ['TaskMentionSearch.tsx', 'MentionList.tsx', 'TaskChip.tsx', 'nodeMapping.tsx', 'useTaskChips.ts', 'extensions/mentionItems.ts']) {
    expect(await source(name)).not.toContain('@tiptap/')
  }
})

test('the starter kit is limited to the allowlisted heading levels', async () => {
  const extensions = await source('extensions/editorExtensions.ts')

  expect(extensions).toContain('levels: [1, 2, 3]')
})

test('editor.css carries the ProseMirror base rules because CSP blocks the injected stylesheet', async () => {
  const css = await Bun.file(new URL('./editor.css', import.meta.url)).text()

  expect(css).toContain('.ProseMirror')
  expect(css).toContain('white-space: pre-wrap')
  expect(css).toContain('word-wrap: break-word')
  expect(css).toContain('.ProseMirror-hideselection')
  expect(css).toContain('.ProseMirror-gapcursor')
  expect(css).toContain('prefers-reduced-motion')
})

test('the empty-editor placeholder is styled through a data attribute, not injected CSS', async () => {
  const css = await Bun.file(new URL('./editor.css', import.meta.url)).text()

  expect(css).toContain('attr(data-placeholder)')
})

test('the editor never tries to inject its stylesheet, because CSP would block it', async () => {
  // TipTap core defaults injectCSS to true and appends a <style> element on every
  // mount. Under `style-src 'self'` that is a CSP violation per editor instance.
  // editor.css ships the same rules instead.
  const [surface, css] = await Promise.all([
    Bun.file(new URL('./RichTextEditorSurface.tsx', import.meta.url)).text(),
    Bun.file(new URL('./editor.css', import.meta.url)).text(),
  ])

  expect(surface).toContain('injectCSS: false')
  expect(css).toContain('img.ProseMirror-separator')
})
