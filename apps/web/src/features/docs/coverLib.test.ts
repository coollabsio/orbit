import { expect, test } from 'bun:test'
import { dragCoverPos, formatCoverPos, parseCoverPos, validCoverUrl } from './coverLib'

test('only http(s) cover URLs are accepted (the server rejects everything else)', () => {
  expect(validCoverUrl(' https://example.com/a.png ')).toBe('https://example.com/a.png')
  expect(validCoverUrl('http://example.com/a.png')).toBe('http://example.com/a.png')
  for (const url of ['javascript:alert(1)', 'data:image/png;base64,AAAA', 'blob:http://x/1', '/a.png', 'example.com/a.png', '', `https://x.io/${'a'.repeat(2048)}`]) {
    expect(validCoverUrl(url)).toBeNull()
  }
})

test('cover positions round-trip as "x,y" percentages and clamp while dragging', () => {
  expect(parseCoverPos(null)).toEqual({ x: 50, y: 50 })
  expect(parseCoverPos('10,120')).toEqual({ x: 10, y: 100 })
  expect(formatCoverPos({ x: 12.345, y: 0 })).toBe('12.3,0')
  expect(dragCoverPos({ x: 50, y: 50 }, 100, 0, { w: 100, h: 0 })).toEqual({ x: 0, y: 50 })
})
