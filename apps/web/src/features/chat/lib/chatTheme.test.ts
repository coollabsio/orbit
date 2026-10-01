import { expect, test } from 'bun:test'
import { accentOn, chatSidebarVariables, chatThemeVariables, normalizeHex, parseChatTheme, readableOn, serializeChatTheme } from './chatTheme'

test('hex colours are normalized, and other text is refused', () => {
  expect(normalizeHex('#ABC')).toBe('#aabbcc')
  expect(normalizeHex('3F0E40')).toBe('#3f0e40')
  expect(normalizeHex(' #1164a3 ')).toBe('#1164a3')
  expect(normalizeHex('#12')).toBeNull()
  expect(normalizeHex('red')).toBeNull()
})

test('a theme code round-trips, with commas or spaces', () => {
  const theme = { sidebar: '#3f0e40', selected: '#1164a3', accent: '#ecb22e', chat: '#ffffff' }
  expect(parseChatTheme(serializeChatTheme(theme))).toEqual(theme)
  expect(parseChatTheme('#3F0E40  1164A3, #ecb22e #fff')).toEqual(theme)
})

test('a three-colour code is a sidebar code: the message surface agrees with the sidebar', () => {
  expect(parseChatTheme('#3f0e40,#1164a3,#ecb22e')?.chat).toBe('#1a1d21')
  expect(parseChatTheme('#f4f1ea,#e0d9c8,#c2410c')?.chat).toBe('#ffffff')
})

test('a code that is not three or four colours is refused', () => {
  expect(parseChatTheme('#3f0e40,#1164a3')).toBeNull()
  expect(parseChatTheme('#3f0e40,#1164a3,#ecb22e,#ffffff,#000000')).toBeNull()
  expect(parseChatTheme('#3f0e40,blue,#ecb22e')).toBeNull()
  expect(parseChatTheme('#3f0e40,#1164a3,#ecb22e,nope')).toBeNull()
  expect(parseChatTheme('')).toBeNull()
})

test('text is white on dark colours and dark on light colours', () => {
  expect(readableOn('#3f0e40')).toBe('#ffffff')
  expect(readableOn('#000000')).toBe('#ffffff')
  expect(readableOn('#f4f1ea')).toBe('#171717')
  expect(readableOn('#ecb22e')).toBe('#171717')
})

test('an accent that is too close to the surface moves toward the text colour', () => {
  // Slack's yellow is unreadable as text on white
  expect(accentOn('#ffffff', '#ecb22e')).not.toBe('#ecb22e')
  // and is kept where it already has contrast
  expect(accentOn('#1a1d21', '#ecb22e')).toBe('#ecb22e')
  expect(accentOn('#ffffff', '#5865f2')).toBe('#5865f2')
})

test('the page and the sidebar get their own surface and readable text', () => {
  const theme = { sidebar: '#f4f1ea', selected: '#3f0e40', accent: '#ecb22e', chat: '#1a1d21' }
  const page = chatThemeVariables(theme) as Record<string, string>
  const sidebar = chatSidebarVariables(theme) as Record<string, string>
  expect(page['--background']).toBe('#1a1d21')
  expect(page['--foreground']).toBe('#ffffff')
  expect(page['--chat-selected']).toBeUndefined()
  expect(sidebar['--background']).toBe('#f4f1ea')
  expect(sidebar['--foreground']).toBe('#171717')
  expect(sidebar['--chat-selected-foreground']).toBe('#ffffff')
  expect(sidebar['--primary-foreground']).toBe('#171717')
  expect(chatThemeVariables(null)).toEqual({})
  expect(chatSidebarVariables(null)).toEqual({})
})
