import { expect, test } from 'bun:test'
import { isOutsideUrl, mediaPreload, proxiedImageUrl } from './outsideUrl'

const origin = 'https://orbit.example'

test('only an http(s) URL on another origin is an outside URL', () => {
  expect(isOutsideUrl('https://tracker.example/p.png', origin)).toBe(true)
  expect(isOutsideUrl('HTTP://tracker.example/p.png', origin)).toBe(true)
  expect(isOutsideUrl('https://orbit.example/api/v1/files/1', origin)).toBe(false)
  expect(isOutsideUrl('/api/v1/workspaces/w/tasks/t/attachments/a/download', origin)).toBe(false)
  expect(isOutsideUrl('blob:https://orbit.example/123', origin)).toBe(false)
  expect(isOutsideUrl('data:image/png;base64,AAAA', origin)).toBe(false)
  expect(isOutsideUrl('', origin)).toBe(false)
})

test('an outside image loads through the proxy with its whole URL; other URLs stay', () => {
  expect(proxiedImageUrl('https://tracker.example/p.png?a=1&b=2#x', origin)).toBe(
    '/api/v1/image?u=https%3A%2F%2Ftracker.example%2Fp.png%3Fa%3D1%26b%3D2%23x',
  )
  expect(proxiedImageUrl('/api/v1/files/1', origin)).toBe('/api/v1/files/1')
  expect(proxiedImageUrl('https://orbit.example/a.png', origin)).toBe('https://orbit.example/a.png')
})

test('an outside video or audio loads nothing before play', () => {
  expect(mediaPreload('https://tracker.example/v.mp4', origin)).toBe('none')
  expect(mediaPreload('/api/v1/files/v.mp4', origin)).toBe('metadata')
})
