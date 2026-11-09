import { expect, test } from 'bun:test'
import { act, render, waitFor } from '@testing-library/react'
import { TextSelection } from '@tiptap/pm/state'
import type { Editor } from '@tiptap/core'
import { PageEditor, type PageEditorProps } from './PageEditor'

// Regression: a comment's Send locks the field (`editable` false, then true). BlockNote re-mounted its view for that,
// and with the cursor on a link its link toolbar read the missing view and threw, which blanked the whole page.
test('changing `editable` with the cursor on a link keeps the editor', async () => {
  const props: PageEditorProps = {
    pageId: 'comment',
    profile: 'task',
    ariaLabel: 'Comment',
    onChange: () => {},
    initialContent: [{ type: 'paragraph', content: [{ type: 'link', href: 'https://example.com', content: [{ type: 'text', text: 'example', styles: {} }] }] }],
    editable: true,
  }
  const view = render(<PageEditor {...props} />)
  await waitFor(() => expect(view.container.querySelector('.bn-editor')).toBeTruthy())
  // Tiptap keeps its editor on the view's element; put the cursor inside the link.
  const tiptap = (view.container.querySelector('.bn-editor') as HTMLElement & { editor: Editor }).editor
  act(() => {
    tiptap.view.dispatch(tiptap.state.tr.setSelection(TextSelection.create(tiptap.state.doc, 5)))
  })

  view.rerender(<PageEditor {...props} editable={false} />)
  expect(view.container.querySelector('.bn-editor')?.getAttribute('contenteditable')).toBe('false')
  // the editor element keeps what the mount gave it (BlockNote's UI needs it focusable)
  const element = view.container.querySelector('.bn-editor')
  expect([element?.getAttribute('tabindex'), element?.getAttribute('role'), element?.getAttribute('aria-label')]).toEqual(['0', 'textbox', 'Comment'])
  view.rerender(<PageEditor {...props} editable />)
  expect(view.container.querySelector('.bn-editor')?.getAttribute('contenteditable')).toBe('true')
})
