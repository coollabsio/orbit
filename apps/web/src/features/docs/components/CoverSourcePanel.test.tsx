import { describe, expect, mock, test } from 'bun:test'
import { act, fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { CoverSourcePanel } from './CoverSourcePanel'

const FILE_URL = '/api/v1/workspaces/w/pages/p/files/f'

function pick(view: ReturnType<typeof render>, file: File) {
  const input = view.getByLabelText('Cover image file') as HTMLInputElement
  Object.defineProperty(input, 'files', { value: [file], configurable: true })
  fireEvent.change(input)
}

describe('CoverSourcePanel', () => {
  test('uploads the chosen image and picks its page file URL', async () => {
    const onPicked = mock((url: string) => void url)
    const onUpload = mock(async (file: File) => {
      void file
      return FILE_URL
    })
    const view = render(<CoverSourcePanel onPicked={onPicked} onUpload={onUpload} />)
    const input = view.getByLabelText('Cover image file') as HTMLInputElement
    expect(input.accept).toContain('image/png')
    expect(input.accept).not.toContain('svg')
    expect(view.getByRole('button', { name: 'Upload image' })).toBeTruthy()

    await act(async () => pick(view, new File(['png'], 'cover.png', { type: 'image/png' })))
    expect(onUpload).toHaveBeenCalledTimes(1)
    expect(onUpload.mock.calls[0][0].name).toBe('cover.png')
    expect(onPicked).toHaveBeenCalledWith(FILE_URL)
  })

  test('shows the upload error and keeps the panel open', async () => {
    const onPicked = mock((url: string) => void url)
    const onUpload = mock(async () => {
      throw new Error('The file is too large to upload.')
    })
    const view = render(<CoverSourcePanel onPicked={onPicked} onUpload={onUpload} />)
    await act(async () => pick(view, new File(['big'], 'big.png', { type: 'image/png' })))
    expect(await view.findByText('The file is too large to upload.')).toBeTruthy()
    expect(onPicked).not.toHaveBeenCalled()
    expect((view.getByRole('button', { name: 'Upload image' }) as HTMLButtonElement).disabled).toBe(false)
  })

  test('the URL option stays, and without an uploader only the URL field is offered', async () => {
    const onPicked = mock((url: string) => void url)
    const view = render(<CoverSourcePanel onPicked={onPicked} />)
    expect(view.queryByRole('button', { name: 'Upload image' })).toBeNull()
    await userEvent.type(view.getByLabelText('Image URL'), 'https://example.com/c.png')
    fireEvent.click(view.getByRole('button', { name: 'Use' }))
    expect(onPicked).toHaveBeenCalledWith('https://example.com/c.png')
  })
})
