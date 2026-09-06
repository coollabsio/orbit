import { expect, test } from 'bun:test'
import { applyVisibleViewport, visibleViewportHeight } from './visibleViewport'

test('visible height prefers the visual viewport so Safari chrome is excluded', () => {
  expect(visibleViewportHeight({ height: 612.4 }, 844)).toBe(612)
  expect(visibleViewportHeight(null, 740)).toBe(740)
})

test('the app height CSS variable is the visible viewport in pixels', () => {
  const properties = new Map<string, string>()
  applyVisibleViewport(
    { style: { setProperty: (name, value) => { properties.set(name, value) } } },
    612,
  )
  expect(properties.get('--app-height')).toBe('612px')
})
