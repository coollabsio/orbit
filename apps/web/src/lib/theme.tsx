import { useCallback, useEffect, useState, useSyncExternalStore } from 'react'
import { customThemeVariables, customThemeScheme, setCustomTheme, useCustomTheme } from '@/lib/customTheme'
import { FONTS, TEXT_SIZES, ThemeContext, type Appearance, type Theme } from './themeContext'

/** The stored value when it is one of `values`, else `fallback`. */
function storedChoice<T extends string>(key: string, values: readonly T[], fallback: T): T {
  const value = localStorage.getItem(key)
  return values.find((choice) => choice === value) ?? fallback
}

// index.html reads the same keys before the first paint.
function storedAppearance(): Appearance {
  const theme = localStorage.getItem('theme')
  return {
    theme: theme === 'light' || theme === 'system' ? theme : 'dark',
    style: localStorage.getItem('orbit:style') === 'flat' ? 'flat' : '3d',
    navStyle: localStorage.getItem('orbit:nav-style') === 'floating' ? 'floating' : 'default',
    font: storedChoice('orbit:font', FONTS, 'noto-sans'),
    textSize: storedChoice('orbit:text-size', TEXT_SIZES, 'default'),
  }
}

const darkQuery = () => window.matchMedia('(prefers-color-scheme: dark)')

function subscribeSystemTheme(listener: () => void) {
  const query = darkQuery()
  query.addEventListener('change', listener)
  return () => query.removeEventListener('change', listener)
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [appearance, setStored] = useState<Appearance>(storedAppearance)
  const systemTheme = useSyncExternalStore<Theme>(subscribeSystemTheme, () => (darkQuery().matches ? 'dark' : 'light'))
  // Account settings → Appearance → Custom replaces the chosen theme: its message colour decides dark or light.
  const customTheme = useCustomTheme()
  const theme = customTheme ? customThemeScheme(customTheme) : appearance.theme === 'system' ? systemTheme : appearance.theme

  useEffect(() => {
    const root = document.documentElement
    root.classList.toggle('dark', theme === 'dark')
    root.toggleAttribute('data-flat', appearance.style === 'flat')
    root.toggleAttribute('data-floating', appearance.navStyle === 'floating')
    root.dataset.font = appearance.font
    root.dataset.textSize = appearance.textSize
    localStorage.setItem('theme', appearance.theme)
    localStorage.setItem('orbit:style', appearance.style)
    localStorage.setItem('orbit:nav-style', appearance.navStyle)
    localStorage.setItem('orbit:font', appearance.font)
    localStorage.setItem('orbit:text-size', appearance.textSize)
  }, [theme, appearance])

  useEffect(() => {
    if (!customTheme) return
    const root = document.documentElement
    const variables = Object.entries(customThemeVariables(customTheme)) as [string, string][]
    for (const [name, value] of variables) root.style.setProperty(name, value)
    return () => {
      for (const [name] of variables) root.style.removeProperty(name)
    }
  }, [customTheme])

  const setAppearance = useCallback((change: Partial<Appearance>) => {
    setStored((current) => ({ ...current, ...change }))
  }, [])

  const toggleTheme = useCallback(() => {
    setCustomTheme(null)
    setAppearance({ theme: theme === 'dark' ? 'light' : 'dark' })
  }, [theme, setAppearance])

  return <ThemeContext value={{ theme, appearance, setAppearance, toggleTheme }}>{children}</ThemeContext>
}
