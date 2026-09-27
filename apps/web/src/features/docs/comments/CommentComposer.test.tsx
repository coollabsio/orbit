// The comment field: a textarea-like BlockNote comment editor with Cancel and a primary submit button.
import { afterEach, beforeAll, describe, expect, mock, test } from 'bun:test'
import { act, cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { BlockNoteEditor } from '@blocknote/core'
import { CommentEditorSubmitExtension } from '@blocknote/core/comments'
import { CommentComposer } from './CommentComposer'
import { commentEditorSchema } from './mentions'

afterEach(cleanup)

beforeAll(() => {
  const meta = document.createElement('meta')
  meta.name = 'viewport'
  meta.content = 'width=device-width, initial-scale=1, interactive-widget=resizes-content'
  document.head.append(meta)
})

const MAC = /Mac|iP(hone|[oa]d)/.test(navigator.platform)

function renderComposer() {
  const onSubmit = mock(async () => {})
  const onCancel = mock(() => {})
  const editor = BlockNoteEditor.create({
    schema: commentEditorSchema,
    trailingBlock: false,
    extensions: [CommentEditorSubmitExtension({ submitOnEnter: false, onSubmit })],
    _tiptapOptions: { injectCSS: false },
  })
  const view = render(<CommentComposer editor={editor} kind="new" submitLabel="Comment" label="New comment" onCancel={onCancel} />)
  const submit = () => view.getByRole('button', { name: 'Comment' }) as HTMLButtonElement
  const field = () => view.container.querySelector('.orbit-comment-input .bn-editor') as HTMLElement
  const type = async (text: string) => {
    await act(async () => {
      editor.focus()
      editor.insertInlineContent(text)
    })
  }
  return { view, editor, onSubmit, onCancel, submit, field, type }
}

describe('CommentComposer', () => {
  test('is a labelled field with Cancel and a submit button that stays disabled while empty', async () => {
    const { view, submit, field, type } = renderComposer()
    await waitFor(() => expect(field()).toBeTruthy())
    expect(view.getByRole('group', { name: 'New comment' })).toBeTruthy()
    expect(view.getByRole('button', { name: 'Cancel' })).toBeTruthy()
    expect(view.getByRole('button', { name: 'Mention someone' })).toBeTruthy()
    expect(field().closest('.orbit-comment-input--new')).toBeTruthy()
    expect(submit().disabled).toBe(true)
    await type('Looks good')
    await waitFor(() => expect(submit().disabled).toBe(false))
  })

  test('the button and Cmd/Ctrl+Enter submit; plain Enter does not', async () => {
    const { onSubmit, submit, field, type } = renderComposer()
    await waitFor(() => expect(field()).toBeTruthy())
    await type('First')
    await waitFor(() => expect(submit().disabled).toBe(false))
    fireEvent.keyDown(field(), { key: 'Enter', code: 'Enter', keyCode: 13 })
    expect(onSubmit).not.toHaveBeenCalled()
    fireEvent.keyDown(field(), { key: 'Enter', code: 'Enter', keyCode: 13, metaKey: MAC, ctrlKey: !MAC })
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    fireEvent.click(submit())
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(2))
  })

  test('Escape and Cancel cancel', async () => {
    const { view, onCancel, field } = renderComposer()
    await waitFor(() => expect(field()).toBeTruthy())
    fireEvent.keyDown(field(), { key: 'Escape', code: 'Escape', keyCode: 27 })
    expect(onCancel).toHaveBeenCalledTimes(1)
    fireEvent.click(view.getByRole('button', { name: 'Cancel' }))
    expect(onCancel).toHaveBeenCalledTimes(2)
  })
})
