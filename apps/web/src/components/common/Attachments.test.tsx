import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { waitForAbsence } from '@/test/waitForAbsence'
import type { Attachment } from '@/lib/attachmentLib'
import { Attachments } from './Attachments'

const image: Attachment = {
  id: 'image-1',
  fileName: 'diagram.png',
  mimeType: 'image/png',
  fileSize: 1024,
  url: 'data:image/png;base64,AA==',
}

test('opens an image in a modal and closes it from the backdrop', async () => {
  const view = render(<Attachments attachments={[image]} />)

  fireEvent.click(view.getByRole('button', { name: 'Open image diagram.png' }))

  const dialog = view.getByRole('dialog', { name: 'diagram.png' })
  expect(dialog.querySelector('img')?.getAttribute('src')).toBe(image.url)
  fireEvent.click(view.getByRole('button', { name: 'Close image viewer' }))
  // the viewer unmounts after its exit animation
  await waitForAbsence(() => view.queryByRole('dialog'))
})

test('the image viewer uses the usable screen height on a phone', () => {
  const view = render(<Attachments attachments={[image]} />)
  fireEvent.click(view.getByRole('button', { name: 'Open image diagram.png' }))

  const dialog = view.getByRole('dialog', { name: 'diagram.png' })
  expect(dialog.className).toContain('h-[var(--app-height,100svh)]')
  expect(dialog.className).toContain('pt-[env(safe-area-inset-top,0px)]')
  expect(dialog.className).toContain('pb-[env(safe-area-inset-bottom,0px)]')
  const img = dialog.querySelector('img')
  expect(img?.className).toContain('max-h-full')
  expect(img?.className).not.toContain('100vh')
})

test('closes the image modal with Escape', async () => {
  const view = render(<Attachments attachments={[image]} />)
  fireEvent.click(view.getByRole('button', { name: 'Open image diagram.png' }))

  fireEvent.keyDown(document, { key: 'Escape' })

  await waitForAbsence(() => view.queryByRole('dialog'))
})

test('a single image with a known size holds its place before it loads, at most 300px high', () => {
  const view = render(<Attachments attachments={[{ ...image, width: 1200, height: 600 }]} />)
  const img = view.getByRole('button', { name: 'Open image diagram.png' }).querySelector('img')!
  expect(img.getAttribute('width')).toBe('1200')
  expect(img.getAttribute('height')).toBe('600')
  expect(img.style.aspectRatio).toBe('1200 / 600')
  expect(img.style.width).toBe('600px')
  view.unmount()

  // a small image is not scaled up, and an image of unknown size sizes itself as before
  const small = render(<Attachments attachments={[{ ...image, width: 80, height: 40 }]} />)
  expect(small.getByRole('button', { name: 'Open image diagram.png' }).querySelector('img')!.style.width).toBe('80px')
  small.unmount()
  const unknown = render(<Attachments attachments={[image]} />)
  expect(unknown.getByRole('button', { name: 'Open image diagram.png' }).querySelector('img')!.style.width).toBe('')
})
