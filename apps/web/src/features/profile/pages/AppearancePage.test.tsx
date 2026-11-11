import { afterEach, expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { ThemeProvider } from '@/lib/theme'
import { AppearancePage } from './AppearancePage'

afterEach(() => {
  localStorage.clear()
  const root = document.documentElement
  root.className = ''
  for (const name of ['data-flat', 'data-floating', 'data-new-ui', 'data-font', 'data-text-size']) root.removeAttribute(name)
})

test('a font tile draws its sample in that font and picking it changes the font of the whole page', () => {
  const view = render(<ThemeProvider><AppearancePage /></ThemeProvider>)
  const notoSans = view.getByRole('button', { name: 'Noto Sans' })
  const literata = view.getByRole('button', { name: 'Literata' })
  expect(notoSans.getAttribute('aria-pressed')).toBe('true')
  expect(literata.querySelector('[data-font="literata"]')?.textContent).toBe('Ag')

  fireEvent.click(literata)

  expect(literata.getAttribute('aria-pressed')).toBe('true')
  expect(notoSans.getAttribute('aria-pressed')).toBe('false')
  expect(document.documentElement.dataset.font).toBe('literata')
  expect(localStorage.getItem('orbit:font')).toBe('literata')
})

test('a text size scales the page, and pressing the chosen size again keeps it', () => {
  const view = render(<ThemeProvider><AppearancePage /></ThemeProvider>)
  const group = view.getByRole('group', { name: 'Text size' })
  const defaultSize = view.getByRole('button', { name: 'Default' })
  const large = view.getByRole('button', { name: 'Large' })
  expect(group.contains(large)).toBe(true)
  expect(defaultSize.getAttribute('aria-pressed')).toBe('true')

  fireEvent.click(large)
  expect(large.getAttribute('aria-pressed')).toBe('true')
  expect(document.documentElement.dataset.textSize).toBe('large')
  expect(localStorage.getItem('orbit:text-size')).toBe('large')

  fireEvent.click(large)
  expect(large.getAttribute('aria-pressed')).toBe('true')
  expect(document.documentElement.dataset.textSize).toBe('large')
})
