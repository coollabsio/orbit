import { describe, expect, it } from 'bun:test'
import { pdfRenderScale } from './attachmentLib'

describe('pdfRenderScale', () => {
  it('draws at the screen density, up to 2×', () => {
    expect(pdfRenderScale(600, { width: 600, height: 800 }, 1)).toBe(1)
    expect(pdfRenderScale(600, { width: 600, height: 800 }, 2)).toBe(2)
    expect(pdfRenderScale(600, { width: 600, height: 800 }, 3)).toBe(2)
  })

  it('keeps a wide page under the canvas size limit', () => {
    expect(pdfRenderScale(4000, { width: 1000, height: 1400 }, 2)).toBe(4096 / 1400)
  })

  it('keeps a tall page under the canvas size limit', () => {
    const scale = pdfRenderScale(900, { width: 600, height: 6000 }, 2)
    expect(scale).toBe(4096 / 6000)
    expect(600 * scale).toBeLessThanOrEqual(4096)
    expect(6000 * scale).toBeLessThanOrEqual(4096)
  })

  it('falls back to 1 before the sizes are known', () => {
    expect(pdfRenderScale(0, { width: 600, height: 800 }, 2)).toBe(1)
    expect(pdfRenderScale(600, { width: 0, height: 800 }, 2)).toBe(1)
    expect(pdfRenderScale(600, { width: 600, height: 0 }, 2)).toBe(1)
  })
})
