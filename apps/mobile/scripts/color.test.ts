import { expect, test } from 'bun:test'

import { cssColorToRn } from './color'

const resolve = (name: string) => ({ primary: 'oklch(0.659 0.216 358.989)' })[name]!

test('converts white and black', () => {
  expect(cssColorToRn('oklch(1 0 0)')).toBe('#ffffff')
  expect(cssColorToRn('oklch(0 0 0)')).toBe('#000000')
})

test('converts Orbit pink (matches lightningcss)', () => {
  expect(cssColorToRn('oklch(0.659 0.216 358.989)')).toBe('#f2458f')
  expect(cssColorToRn('oklch(0.145 0 0)')).toBe('#0a0a0a')
})

test('clamps out-of-gamut channels', () => {
  expect(cssColorToRn('oklch(0.577 0.245 27.325)')).toBe('#e7000b')
})

test('keeps alpha as rgba', () => {
  expect(cssColorToRn('oklch(1 0 0 / 10%)')).toBe('rgba(255, 255, 255, 0.1)')
  expect(cssColorToRn('rgb(0 0 0 / 6%)')).toBe('rgba(0, 0, 0, 0.06)')
  expect(cssColorToRn('transparent')).toBe('rgba(0, 0, 0, 0)')
})

test('resolves var() and mixes in oklab', () => {
  expect(cssColorToRn('var(--primary)', resolve)).toBe('#f2458f')
  expect(cssColorToRn('color-mix(in oklab, var(--primary) 10%, oklch(0.2002 0 0))', resolve)).toBe('#2a1c20')
})
