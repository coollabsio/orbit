import { describe, expect, test } from 'bun:test'

import { alpha, avatarColors, mix, splitAlpha, userColor } from './color'

// Expected values are what Chromium paints for the web's CSS with the tokens as authored (`oklch(0.269 0 0)`). The app
// gets the tokens as rounded hex, so a channel may differ by one step.
const channels = (hex: string) => [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16))
const expectClose = (actual: string, expected: string) => {
  const diff = channels(actual).map((value, index) => Math.abs(value - channels(expected)[index]!))
  expect({ color: actual, close: Math.max(...diff) <= 1 }).toEqual({ color: actual, close: true })
  expect(actual).toMatch(/^#[0-9a-f]{6}$/)
}
describe('userColor', () => {
  test('hashes the id to the web hue', () => {
    expect(userColor('01a1411f-efb1-7423-a0cb-007aedf8c8c0')).toBe('hsl(324, 55%, 48%)')
  })
})

describe('mix', () => {
  test('matches color-mix(in oklch) against a gray token, whose hue is 0', () => {
    expectClose(mix('hsl(324, 55%, 48%)', 0.26, '#262626'), '#4e2e3a')
    expectClose(mix('hsl(79, 55%, 48%)', 0.26, '#262626'), '#5b3d37')
  })

  test('mixes toward the foreground for the initials', () => {
    expectClose(mix('hsl(324, 55%, 48%)', 0.6, '#fafafa'), '#df89ae')
    expectClose(mix('hsl(79, 55%, 48%)', 0.6, '#fafafa'), '#f1c280')
    expectClose(mix('hsl(324, 55%, 48%)', 0.6, '#0a0a0a'), '#71274c')
  })

  test('darkens the primary for the button edge', () => {
    expectClose(mix('#f2458f', 0.75, '#000000'), '#a52c60')
  })
})

describe('avatarColors', () => {
  test('tints the light muted surface', () => {
    const look = avatarColors('hsl(324, 55%, 48%)', '#f5f5f5', '#0a0a0a')
    expectClose(look.background, '#eec6d3')
    expectClose(look.text, '#71274c')
    expectClose(avatarColors('hsl(79, 55%, 48%)', '#f5f5f5', '#0a0a0a').background, '#fdd8d0')
  })
})

describe('alpha', () => {
  test('scales a hex or rgba color', () => {
    expect(alpha('#a1a1a1', 0.7)).toBe('rgba(161, 161, 161, 0.7)')
    expect(alpha('rgba(255, 255, 255, 0.15)', 0.3)).toBe('rgba(255, 255, 255, 0.045)')
  })
})

test('splitAlpha gives the opaque color and its opacity, for SVG gradient stops', () => {
  expect(splitAlpha('rgba(242, 69, 143, 0.09)')).toEqual({ color: '#f2458f', opacity: 0.09 })
  expect(splitAlpha('#160c0f')).toEqual({ color: '#160c0f', opacity: 1 })
  expect(splitAlpha('rgba(0, 0, 0, 0)').opacity).toBe(0)
})
