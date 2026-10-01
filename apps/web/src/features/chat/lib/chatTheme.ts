import { useSyncExternalStore, type CSSProperties } from 'react'

/**
 * A personal colour theme for the chat sidebar, as in Slack: three colours, and the text colours are derived so that
 * they stay readable. The reading surface (messages) is never themed. `null` is the Orbit default.
 */
export interface ChatTheme {
  /** The sidebar background. */
  sidebar: string
  /** The open conversation's row. */
  selected: string
  /** Unread markers and count badges. */
  accent: string
}

export interface ChatThemePreset {
  name: string
  theme: ChatTheme
}

export const CHAT_THEME_PRESETS: readonly ChatThemePreset[] = [
  { name: 'Aubergine', theme: { sidebar: '#3f0e40', selected: '#1164a3', accent: '#ecb22e' } },
  { name: 'Midnight', theme: { sidebar: '#101828', selected: '#2e4a7d', accent: '#f2458f' } },
  { name: 'Forest', theme: { sidebar: '#12342b', selected: '#2c6e57', accent: '#f4b860' } },
  { name: 'Ember', theme: { sidebar: '#3b1410', selected: '#8f2d1f', accent: '#ffb86b' } },
  { name: 'Slate', theme: { sidebar: '#2b3038', selected: '#4a5361', accent: '#6cc4f5' } },
  { name: 'Paper', theme: { sidebar: '#f4f1ea', selected: '#e0d9c8', accent: '#c2410c' } },
  { name: 'Blossom', theme: { sidebar: '#fde8f1', selected: '#f9c4dc', accent: '#d61f69' } },
]

const STORAGE_KEY = 'orbit:chat:theme'
const HEX = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i

/** A colour as `#rrggbb` in lower case, or `null` when the text is not a hex colour. */
export function normalizeHex(text: string): string | null {
  const match = HEX.exec(text.trim())
  if (!match) return null
  const digits = match[1].toLowerCase()
  return `#${digits.length === 3 ? [...digits].map((digit) => digit + digit).join('') : digits}`
}

/** WCAG relative luminance of a `#rrggbb` colour. */
function luminance(hex: string): number {
  const channel = (offset: number) => {
    const value = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
}

const LIGHT_TEXT = '#ffffff'
const DARK_TEXT = '#171717'

/** White or near-black, whichever has more contrast on this background. */
export function readableOn(background: string): string {
  const level = luminance(background)
  const onLight = (level + 0.05) / (luminance(DARK_TEXT) + 0.05)
  const onDark = (luminance(LIGHT_TEXT) + 0.05) / (level + 0.05)
  return onDark >= onLight ? LIGHT_TEXT : DARK_TEXT
}

/** The theme as a code to share, as in Slack: `#3f0e40,#1164a3,#ecb22e`. */
export function serializeChatTheme(theme: ChatTheme): string {
  return [theme.sidebar, theme.selected, theme.accent].join(',')
}

/** Reads a shared code: three hex colours separated by commas or spaces. `null` when it is not one. */
export function parseChatTheme(code: string): ChatTheme | null {
  const colors = code.split(/[\s,]+/).filter(Boolean).map(normalizeHex)
  const [sidebar, selected, accent] = colors
  if (colors.length !== 3 || !sidebar || !selected || !accent) return null
  return { sidebar, selected, accent }
}

/**
 * The token overrides for the sidebar element. Its children use the shadcn tokens (`bg-background`, `text-foreground`,
 * `bg-muted`, `bg-primary`…), so setting the variables here re-colours all of them. Menus and dialogs are portalled
 * out of the sidebar and keep the app theme.
 */
export function chatThemeVariables(theme: ChatTheme | null): CSSProperties {
  if (!theme) return {}
  const text = readableOn(theme.sidebar)
  return {
    '--background': theme.sidebar,
    '--foreground': text,
    '--muted': `color-mix(in oklch, ${text} 12%, ${theme.sidebar})`,
    '--muted-foreground': `color-mix(in oklch, ${text} 68%, ${theme.sidebar})`,
    '--border': `color-mix(in oklch, ${text} 14%, transparent)`,
    '--ring': `color-mix(in oklch, ${text} 50%, ${theme.sidebar})`,
    '--primary': theme.accent,
    '--primary-foreground': readableOn(theme.accent),
    '--chat-selected': theme.selected,
    '--chat-selected-foreground': readableOn(theme.selected),
  } as CSSProperties
}

const listeners = new Set<() => void>()
let cached: { raw: string | null; theme: ChatTheme | null } | null = null

function read(): ChatTheme | null {
  let raw: string | null = null
  try {
    raw = globalThis.localStorage?.getItem(STORAGE_KEY) ?? null
  } catch {
    // Unreadable storage: the default theme.
  }
  if (!cached || cached.raw !== raw) cached = { raw, theme: raw ? parseChatTheme(raw) : null }
  return cached.theme
}

/** Saves the theme in this browser (theme sync is a backend project). `null` goes back to the Orbit default. */
export function setChatTheme(theme: ChatTheme | null) {
  try {
    if (theme) globalThis.localStorage?.setItem(STORAGE_KEY, serializeChatTheme(theme))
    else globalThis.localStorage?.removeItem(STORAGE_KEY)
  } catch {
    // Storage is full or blocked: the theme does not change.
  }
  for (const listener of listeners) listener()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  globalThis.addEventListener?.('storage', listener)
  return () => {
    listeners.delete(listener)
    globalThis.removeEventListener?.('storage', listener)
  }
}

export function useChatTheme(): ChatTheme | null {
  return useSyncExternalStore(subscribe, read, () => null)
}
