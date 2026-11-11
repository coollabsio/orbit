import { afterEach, expect, test } from 'bun:test'
import { act, fireEvent, render } from '@testing-library/react'
import { DEFAULT_APP_LOOKS, setAppLook, setCustomTheme, setOpenPage } from '@/lib/customTheme'
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
  setOpenPage(null)
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

test('per app colours docs, chat and tasks each in their own way, and other pages keep the chosen theme', () => {
  const root = document.documentElement
  localStorage.setItem('theme', 'light')
  const view = render(<ThemeProvider><Controls /></ThemeProvider>)
  act(() => setCustomTheme({ apps: DEFAULT_APP_LOOKS }))

  const page = (pathname: string) => {
    act(() => setOpenPage(pathname))
    return [view.getByTestId('theme').textContent, root.style.getPropertyValue('--background')]
  }
  expect(page('/tasks/ORB-1')).toEqual(['dark', '#16171a'])
  expect(page('/views/abc')).toEqual(['dark', '#16171a'])
  expect(page('/docs/page-1')).toEqual(['light', '#ffffff'])
  expect(page('/chat/general')).toEqual(['light', '#ffffff'])
  expect(page('/profile/appearance')).toEqual(['light', ''])
  expect(page('/tasks-trash')).toEqual(['light', ''])
})

test('each part of the app can take other colours, or the chosen theme, and the mix is saved', () => {
  localStorage.setItem('theme', 'light')
  const view = render(<ThemeProvider><Controls /></ThemeProvider>)
  act(() => setAppLook('docs', { sidebar: '#0e0f11', selected: '#24252a', accent: '#5e6ad2', chat: '#16171a' }))
  act(() => setAppLook('chat', null))

  expect(JSON.parse(localStorage.getItem('orbit:chat:theme')!.slice('apps:'.length))).toEqual({
    docs: '#0e0f11,#24252a,#5e6ad2,#16171a',
    chat: null,
    tasks: '#0e0f11,#24252a,#5e6ad2,#16171a',
  })
  act(() => setOpenPage('/docs'))
  expect([view.getByTestId('theme').textContent, document.documentElement.style.getPropertyValue('--background')]).toEqual(['dark', '#16171a'])
  act(() => setOpenPage('/chat'))
  expect([view.getByTestId('theme').textContent, document.documentElement.style.getPropertyValue('--background')]).toEqual(['light', ''])
})
