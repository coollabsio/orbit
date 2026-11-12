import { expect, test } from 'bun:test'
import { isAnimated, MAX_IMAGE_EDGE, shrinkImage, SHRINK_MIN_BYTES, targetSize } from './shrinkImage'

const bytes = (text: string) => new Uint8Array(Array.from(text, (char) => char.charCodeAt(0)))

test('targetSize scales the long edge down and keeps the aspect ratio', () => {
  expect(targetSize(5120, 2880)).toEqual({ width: MAX_IMAGE_EDGE, height: 1440 })
  expect(targetSize(3000, 6000)).toEqual({ width: 1280, height: MAX_IMAGE_EDGE })
})

test('targetSize never enlarges a small image', () => {
  expect(targetSize(800, 600)).toEqual({ width: 800, height: 600 })
})

test('isAnimated finds APNG and animated WebP chunks', () => {
  expect(isAnimated(bytes('\x89PNG\r\n\x1a\n....IHDR........acTL'))).toBe(true)
  expect(isAnimated(bytes('RIFF....WEBPVP8X..........ANIM'))).toBe(true)
  expect(isAnimated(bytes('\x89PNG\r\n\x1a\n....IHDR........IDAT'))).toBe(false)
})

test('shrinkImage keeps small files, PNGs, GIFs and SVGs as they are', async () => {
  const large = new Uint8Array(SHRINK_MIN_BYTES + 1)
  for (const file of [
    new File([new Uint8Array(10)], 'small.jpg', { type: 'image/jpeg' }),
    new File([large], 'screenshot.png', { type: 'image/png' }),
    new File([large], 'cat.gif', { type: 'image/gif' }),
    new File([large], 'logo.svg', { type: 'image/svg+xml' }),
    new File([large], 'notes.pdf', { type: 'application/pdf' }),
  ]) {
    expect(await shrinkImage(file)).toBe(file)
  }
})

test('shrinkImage keeps a file it cannot decode', async () => {
  const file = new File([new Uint8Array(SHRINK_MIN_BYTES + 1)], 'broken.jpg', { type: 'image/jpeg' })
  expect(await shrinkImage(file)).toBe(file)
})
