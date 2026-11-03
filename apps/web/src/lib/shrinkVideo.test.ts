import { expect, test } from 'bun:test'
import { MAX_VIDEO_EDGE, shrinkVideo, SHRINK_VIDEO_MIN_BYTES, videoTargetSize } from './shrinkVideo'

test('videoTargetSize scales 4K down to 1080p and keeps the aspect ratio', () => {
  expect(videoTargetSize(3840, 2160)).toEqual({ width: MAX_VIDEO_EDGE, height: 1080 })
  expect(videoTargetSize(2160, 3840)).toEqual({ width: 1080, height: MAX_VIDEO_EDGE })
})

test('videoTargetSize gives even dimensions and never enlarges', () => {
  expect(videoTargetSize(2560, 1081)).toEqual({ width: MAX_VIDEO_EDGE, height: 810 })
  expect(videoTargetSize(641, 359)).toEqual({ width: 640, height: 358 })
})

test('shrinkVideo keeps small videos and other files as they are', async () => {
  const small = new File([new Uint8Array(1024)], 'clip.mov', { type: 'video/quicktime' })
  const large = new File([new Uint8Array(SHRINK_VIDEO_MIN_BYTES)], 'notes.pdf', { type: 'application/pdf' })
  expect(await shrinkVideo(small)).toBe(small)
  expect(await shrinkVideo(large)).toBe(large)
})
