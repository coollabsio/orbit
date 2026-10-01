import { expect, test } from 'bun:test'
import { chatThemeVariables, normalizeHex, parseChatTheme, readableOn, serializeChatTheme } from './chatTheme'

test('hex colours are normalized, and other text is refused', () => {
  expect(normalizeHex('#ABC')).toBe('#aabbcc')
  expect(normalizeHex('3F0E40')).toBe('#3f0e40')
  expect(normalizeHex(' #1164a3 ')).toBe('#1164a3')
  expect(normalizeHex('#12')).toBeNull()
  expect(normalizeHex('red')).toBeNull()
})

test('a theme code round-trips, with commas or spaces', () => {
  const theme = { sidebar: '#3f0e40', selected: '#1164a3', accent: '#ecb22e' }
  expect(parseChatTheme(serializeChatTheme(theme))).toEqual(theme)
  expect(parseChatTheme('#3F0E40  1164A3, #ecb22e')).toEqual(theme)
})

test('a code that is not three colours is refused', () => {
  expect(parseChatTheme('#3f0e40,#1164a3')).toBeNull()
  expect(parseChatTheme('#3f0e40,#1164a3,#ecb22e,#ffffff')).toBeNull()
  expect(parseChatTheme('#3f0e40,blue,#ecb22e')).toBeNull()
  expect(parseChatTheme('')).toBeNull()
})

test('text is white on dark colours and dark on light colours', () => {
  expect(readableOn('#3f0e40')).toBe('#ffffff')
  expect(readableOn('#000000')).toBe('#ffffff')
  expect(readableOn('#f4f1ea')).toBe('#171717')
  expect(readableOn('#ecb22e')).toBe('#171717')
})

test('the variables carry the three colours and readable text for each', () => {
  const variables = chatThemeVariables({ sidebar: '#f4f1ea', selected: '#3f0e40', accent: '#ecb22e' }) as Record<string, string>
  expect(variables['--background']).toBe('#f4f1ea')
  expect(variables['--foreground']).toBe('#171717')
  expect(variables['--chat-selected-foreground']).toBe('#ffffff')
  expect(variables['--primary-foreground']).toBe('#171717')
  expect(chatThemeVariables(null)).toEqual({})
})
