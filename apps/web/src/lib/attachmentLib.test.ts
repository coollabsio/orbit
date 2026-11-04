import { describe, expect, it } from 'bun:test'
import { pdfRenderScale } from './attachmentLib'

describe('pdfRenderScale', () => {
  it('draws at the screen density, up to 2×', () => {
    expect(pdfRenderScale(600, 600, 1)).toBe(1)
    expect(pdfRenderScale(600, 600, 2)).toBe(2)
    expect(pdfRenderScale(600, 600, 3)).toBe(2)
  })

  it('keeps a wide page under the canvas size limit', () => {
    expect(pdfRenderScale(4000, 1000, 2)).toBe(4096 / 1000)
  })

  it('falls back to 1 before the sizes are known', () => {
    expect(pdfRenderScale(0, 600, 2)).toBe(1)
    expect(pdfRenderScale(600, 0, 2)).toBe(1)
  })
})
