import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import type { Attachment } from '../../../mock/types'
import { Attachments } from './Attachments'

const image: Attachment = {
  id: 'image-1',
  fileName: 'diagram.png',
  mimeType: 'image/png',
  fileSize: 1024,
  url: 'data:image/png;base64,AA==',
}

test('opens an image in a modal and closes it from the backdrop', () => {
  const view = render(<Attachments attachments={[image]} />)

  fireEvent.click(view.getByRole('button', { name: 'Open image diagram.png' }))

  const dialog = view.getByRole('dialog', { name: 'diagram.png' })
  expect(dialog.querySelector('img')?.getAttribute('src')).toBe(image.url)
  fireEvent.click(view.getByRole('button', { name: 'Close image viewer' }))
  expect(view.queryByRole('dialog')).toBeNull()
})

test('closes the image modal with Escape', () => {
  const view = render(<Attachments attachments={[image]} />)
  fireEvent.click(view.getByRole('button', { name: 'Open image diagram.png' }))

  fireEvent.keyDown(window, { key: 'Escape' })

  expect(view.queryByRole('dialog')).toBeNull()
})
