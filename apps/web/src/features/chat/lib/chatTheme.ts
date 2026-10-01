import { useSyncExternalStore, type CSSProperties } from 'react'

/**
 * A personal colour theme for chat: four colours, and every text colour is derived so that it stays readable. `null`
 * is the Orbit default (the app theme).
 */
export interface ChatTheme {
  /** The sidebar background. */
  sidebar: string
  /** The open conversation's row. */
  selected: string
  /** Unread markers, count badges, mentions and the send button. */
  accent: string
  /** The background of messages, the headers and the right pane. */
  chat: string
}

export interface ChatThemePreset {
  name: string
  theme: ChatTheme
}

export const CHAT_THEME_PRESETS: readonly ChatThemePreset[] = [
  { name: 'Slack', theme: { sidebar: '#3f0e40', selected: '#1164a3', accent: '#ecb22e', chat: '#ffffff' } },
  { name: 'Slack Dark', theme: { sidebar: '#19171d', selected: '#1164a3', accent: '#ecb22e', chat: '#1a1d21' } },
  { name: 'Discord Ash', theme: { sidebar: '#2b2d31', selected: '#404249', accent: '#5865f2', chat: '#313338' } },
  { name: 'Discord Dark', theme: { sidebar: '#121214', selected: '#2c2c30', accent: '#5865f2', chat: '#1a1a1e' } },
  { name: 'Discord Onyx', theme: { sidebar: '#000000', selected: '#1f1f23', accent: '#5865f2', chat: '#070709' } },
  { name: 'Discord Light', theme: { sidebar: '#f2f3f5', selected: '#d7d9dc', accent: '#5865f2', chat: '#ffffff' } },
  { name: 'Midnight', theme: { sidebar: '#0f172a', selected: '#2e4a7d', accent: '#f2458f', chat: '#1a2338' } },
  { name: 'Forest', theme: { sidebar: '#12342b', selected: '#2c6e57', accent: '#f4b860', chat: '#1b2a25' } },
  { name: 'Ember', theme: { sidebar: '#3b1410', selected: '#8f2d1f', accent: '#ffb86b', chat: '#241816' } },
  { name: 'Slate', theme: { sidebar: '#2b3038', selected: '#4a5361', accent: '#6cc4f5', chat: '#353b45' } },
  { name: 'Paper', theme: { sidebar: '#ebe6da', selected: '#d9d1bf', accent: '#c2410c', chat: '#f7f4ec' } },
  { name: 'Blossom', theme: { sidebar: '#fde8f1', selected: '#f9c4dc', accent: '#d61f69', chat: '#fff7fa' } },
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

/** WCAG contrast ratio of two colours, from 1 to 21. */
function contrast(a: string, b: string): number {
  const [low, high] = [luminance(a), luminance(b)].sort((x, y) => x - y)
  return (high + 0.05) / (low + 0.05)
}

/** White or near-black, whichever has more contrast on this background. */
export function readableOn(background: string): string {
  return contrast(LIGHT_TEXT, background) >= contrast(DARK_TEXT, background) ? LIGHT_TEXT : DARK_TEXT
}

/** `from` moved toward `to` by `amount` (0 to 1), channel by channel. */
function mix(from: string, to: string, amount: number): string {
  const channel = (offset: number) => {
    const start = Number.parseInt(from.slice(offset, offset + 2), 16)
    const end = Number.parseInt(to.slice(offset, offset + 2), 16)
    return Math.round(start + (end - start) * amount).toString(16).padStart(2, '0')
  }
  return `#${channel(1)}${channel(3)}${channel(5)}`
}

/**
 * The accent as it shows on the message surface (mention text, the "New" line). A sidebar accent can be too close to
 * the surface (Slack's yellow on white), so it moves toward the text colour until it has a 3:1 contrast.
 */
export function accentOn(surface: string, accent: string): string {
  const text = readableOn(surface)
  let color = accent
  for (let step = 0; step < 8 && contrast(color, surface) < 3; step += 1) color = mix(color, text, 0.15)
  return color
}

/** The theme as a code to share: `#3f0e40,#1164a3,#ecb22e,#ffffff` (sidebar, selected, accent, messages). */
export function serializeChatTheme(theme: ChatTheme): string {
  return [theme.sidebar, theme.selected, theme.accent, theme.chat].join(',')
}

/**
 * Reads a shared code: four hex colours separated by commas or spaces. Three colours are a sidebar-only code; the
 * message surface is then a neutral one that agrees with the sidebar. `null` when the text is not a code.
 */
export function parseChatTheme(code: string): ChatTheme | null {
  const colors = code.split(/[\s,]+/).filter(Boolean).map(normalizeHex)
  const [sidebar, selected, accent, chat] = colors
  if (colors.length < 3 || colors.length > 4 || !sidebar || !selected || !accent || (colors.length === 4 && !chat)) return null
  return { sidebar, selected, accent, chat: chat ?? (readableOn(sidebar) === LIGHT_TEXT ? '#1a1d21' : '#ffffff') }
}

/** The shadcn surface tokens for one background colour: text, quiet text, hover, borders and fields. */
function surfaceVariables(surface: string, accent: string): Record<string, string> {
  const text = readableOn(surface)
  const tint = (percent: number) => `color-mix(in oklch, ${text} ${percent}%, ${surface})`
  return {
    '--background': surface,
    '--foreground': text,
    '--card': tint(4),
    '--card-foreground': text,
    '--muted': tint(11),
    '--muted-foreground': tint(66),
    '--secondary': tint(13),
    '--secondary-foreground': text,
    '--accent': tint(11),
    '--accent-foreground': text,
    '--border': `color-mix(in oklch, ${text} 14%, transparent)`,
    '--input': `color-mix(in oklch, ${text} 20%, transparent)`,
    '--ring': tint(50),
    '--primary': accent,
    '--primary-foreground': readableOn(accent),
  }
}

/**
 * The token overrides for the chat page: the message column, the headers and the right pane. Chat components use the
 * shadcn tokens (`bg-background`, `text-foreground`, `bg-muted`, `bg-primary`…), so setting the variables on the root
 * re-colours all of them. Menus, dialogs and sheets are portalled out of the page and keep the app theme.
 */
export function chatThemeVariables(theme: ChatTheme | null): CSSProperties {
  if (!theme) return {}
  return surfaceVariables(theme.chat, accentOn(theme.chat, theme.accent)) as CSSProperties
}

/** The overrides for the sidebar inside the chat page: its own surface, and the colour of the open row. */
export function chatSidebarVariables(theme: ChatTheme | null): CSSProperties {
  if (!theme) return {}
  return {
    ...surfaceVariables(theme.sidebar, theme.accent),
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
