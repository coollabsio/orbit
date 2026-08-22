import { createContext, useContext } from 'react'

export type Theme = 'dark' | 'light'

export const ThemeContext = createContext<{
  theme: Theme
  setTheme: (theme: Theme) => void
  toggleTheme: () => void
}>({
  theme: 'dark',
  setTheme: () => {},
  toggleTheme: () => {},
})

export function useTheme() {
  return useContext(ThemeContext)
}
