import { expect, test } from 'bun:test'

import { accentOn, colorVariables, customSidebarTheme, customThemeColors, customThemeScheme, normalizeHex, parseCustomTheme, readableOn, serializeCustomTheme } from './custom-theme'

const slack = { sidebar: '#3f0e40', selected: '#1164a3', accent: '#ecb22e', chat: '#ffffff' }

test('a shared code round-trips, and three colours get a matching message surface', () => {
  expect(parseCustomTheme(serializeCustomTheme(slack))).toEqual(slack)
  expect(parseCustomTheme('#3F0E40 #1164a3 #ecb22e')?.chat).toBe('#1a1d21')
  expect(parseCustomTheme('#fff,#000')).toBeNull()
  expect(parseCustomTheme('#fff,#000,#abc,nope')).toBeNull()
  expect(normalizeHex(' ABC ')).toBe('#aabbcc')
})

test('text and accents stay readable on the theme colours', () => {
  expect(readableOn('#3f0e40')).toBe('#ffffff')
  expect(readableOn('#fde8f1')).toBe('#171717')
  // Slack's yellow on white is moved until it has contrast; on its dark sidebar it is kept
  expect(accentOn('#ffffff', '#ecb22e')).not.toBe('#ecb22e')
  expect(accentOn('#1a1d21', '#ecb22e')).toBe('#ecb22e')
})

test('the page colours follow the message colour and the drawer follows the sidebar colour', () => {
  expect(customThemeScheme(slack)).toBe('light')
  expect(customThemeColors(slack).background).toBe('#ffffff')
  const drawer = customSidebarTheme(slack)
  expect(drawer.scheme).toBe('dark')
  expect(drawer.colors.sidebar).toBe('#3f0e40')
  expect(drawer.colors.sidebarAccent).toBe('#1164a3')
  expect(drawer.colors.sidebarNavActive).toBe('#ffffff')
})

test('colour tokens become the variables Uniwind reads', () => {
  expect(colorVariables({ sidebarNavActive: '#fff', chart1: '#000' })).toEqual({ '--color-sidebar-nav-active': '#fff', '--color-chart1': '#000' })
})
