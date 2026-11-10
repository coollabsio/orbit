import { afterEach, expect, test } from 'bun:test'
import { act, fireEvent, render } from '@testing-library/react'
import { setCustomTheme } from '@/lib/customTheme'
import { ThemeProvider } from './theme'
import { useTheme } from './themeContext'

function Controls() {
  const { theme, appearance, setAppearance } = useTheme()
  return (
    <>
      <output data-testid="theme">{theme}</output>
      <output data-testid="appearance">{JSON.stringify(appearance)}</output>
      <button type="button" onClick={() => setAppearance({ theme: 'light', style: 'flat', navStyle: 'floating', font: 'literata', textSize: 'large' })}>change</button>
    </>
  )
}

afterEach(() => {
  localStorage.clear()
  document.documentElement.className = ''
  document.documentElement.removeAttribute('style')
  document.documentElement.removeAttribute('data-flat')
  document.documentElement.removeAttribute('data-floating')
  document.documentElement.removeAttribute('data-font')
  document.documentElement.removeAttribute('data-text-size')
  setCustomTheme(null)
})

test('a choice is saved and applied to the page', () => {
  const view = render(<ThemeProvider><Controls /></ThemeProvider>)
  expect(view.getByTestId('appearance').textContent).toBe('{"theme":"dark","style":"3d","navStyle":"default","font":"noto-sans","textSize":"default"}')
  expect(document.documentElement.classList.contains('dark')).toBe(true)
  expect(document.documentElement.dataset.font).toBe('noto-sans')
  expect(document.documentElement.dataset.textSize).toBe('default')

  fireEvent.click(view.getByRole('button', { name: 'change' }))

  expect(view.getByTestId('theme').textContent).toBe('light')
  expect(document.documentElement.classList.contains('dark')).toBe(false)
  expect(document.documentElement.hasAttribute('data-flat')).toBe(true)
  expect(document.documentElement.hasAttribute('data-floating')).toBe(true)
  expect([localStorage.getItem('theme'), localStorage.getItem('orbit:style'), localStorage.getItem('orbit:nav-style')]).toEqual(['light', 'flat', 'floating'])
  expect([localStorage.getItem('orbit:font'), localStorage.getItem('orbit:text-size')]).toEqual(['literata', 'large'])
})

test('a stored font and text size are read back, and unknown ones fall back to the defaults', () => {
  localStorage.setItem('orbit:font', 'inter')
  localStorage.setItem('orbit:text-size', 'compact')
  const view = render(<ThemeProvider><Controls /></ThemeProvider>)
  expect(JSON.parse(view.getByTestId('appearance').textContent ?? '')).toMatchObject({ font: 'inter', textSize: 'compact' })
  view.unmount()

  localStorage.setItem('orbit:font', 'comic-sans')
  localStorage.setItem('orbit:text-size', 'huge')
  const fallback = render(<ThemeProvider><Controls /></ThemeProvider>)
  expect(JSON.parse(fallback.getByTestId('appearance').textContent ?? '')).toMatchObject({ font: 'noto-sans', textSize: 'default' })
  expect(document.documentElement.dataset.font).toBe('noto-sans')
})

test('the system theme follows the operating system', () => {
  const listeners = new Set<() => void>()
  let dark = true
  const original = window.matchMedia
  window.matchMedia = (() => ({
    get matches() { return dark },
    addEventListener: (_: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_: string, listener: () => void) => listeners.delete(listener),
  })) as unknown as typeof window.matchMedia
  try {
    localStorage.setItem('theme', 'system')
    const view = render(<ThemeProvider><Controls /></ThemeProvider>)
    expect(view.getByTestId('theme').textContent).toBe('dark')

    dark = false
    act(() => listeners.forEach((listener) => listener()))

    expect(view.getByTestId('theme').textContent).toBe('light')
    expect(document.documentElement.classList.contains('dark')).toBe(false)
    expect(localStorage.getItem('theme')).toBe('system')
  } finally {
    window.matchMedia = original
  }
})

test('a custom theme replaces the chosen theme on every page, and turning it off restores the theme', () => {
  const root = document.documentElement
  const view = render(<ThemeProvider><Controls /></ThemeProvider>)

  act(() => setCustomTheme({ sidebar: '#ebe6da', selected: '#d9d1bf', accent: '#c2410c', chat: '#f7f4ec' }))
  expect(view.getByTestId('theme').textContent).toBe('light')
  expect(root.classList.contains('dark')).toBe(false)
  expect(root.style.getPropertyValue('--background')).toBe('#f7f4ec')

  act(() => setCustomTheme(null))
  expect(view.getByTestId('theme').textContent).toBe('dark')
  expect(root.classList.contains('dark')).toBe(true)
  expect(root.style.getPropertyValue('--background')).toBe('')
})
