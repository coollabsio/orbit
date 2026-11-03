import type { ColorName, Colors, Scheme } from '../theme'
import { alpha, mix } from './color'

/**
 * Settings → Appearance → Custom: a personal colour theme, the web's `lib/customTheme.ts`. Four colours, and every
 * text colour is derived so that it stays readable. In the new UI it colours the whole app; in the classic layout, chat.
 */
export interface CustomTheme {
  /** The sidebar (drawer) background. */
  sidebar: string
  /** The open row. */
  selected: string
  /** Unread markers, count badges, mentions and the send button. */
  accent: string
  /** The background of every screen. */
  chat: string
}

export interface CustomThemePreset {
  name: string
  theme: CustomTheme
}

export const CUSTOM_THEME_PRESETS: readonly CustomThemePreset[] = [
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

const HEX = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i
const LIGHT_TEXT = '#ffffff'
const DARK_TEXT = '#171717'

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

/** WCAG contrast ratio of two colours, from 1 to 21. */
function contrast(a: string, b: string): number {
  const [low, high] = [luminance(a), luminance(b)].sort((x, y) => x - y)
  return (high + 0.05) / (low + 0.05)
}

/** White or near-black, whichever has more contrast on this background. */
export function readableOn(background: string): string {
  return contrast(LIGHT_TEXT, background) >= contrast(DARK_TEXT, background) ? LIGHT_TEXT : DARK_TEXT
}

/** `from` moved toward `to` by `amount` (0 to 1), channel by channel (the web's sRGB step, not `color-mix`). */
function step(from: string, to: string, amount: number): string {
  const channel = (offset: number) => {
    const start = Number.parseInt(from.slice(offset, offset + 2), 16)
    const end = Number.parseInt(to.slice(offset, offset + 2), 16)
    return Math.round(start + (end - start) * amount).toString(16).padStart(2, '0')
  }
  return `#${channel(1)}${channel(3)}${channel(5)}`
}

/** The accent as it shows on a surface: moved toward the text colour until it has a 3:1 contrast. */
export function accentOn(surface: string, accent: string): string {
  const text = readableOn(surface)
  let color = accent
  for (let index = 0; index < 8 && contrast(color, surface) < 3; index += 1) color = step(color, text, 0.15)
  return color
}

/** The theme as a code to share: `#3f0e40,#1164a3,#ecb22e,#ffffff` (sidebar, selected, accent, messages). */
export function serializeCustomTheme(theme: CustomTheme): string {
  return [theme.sidebar, theme.selected, theme.accent, theme.chat].join(',')
}

/** Reads a shared code: three or four hex colours. `null` when the text is not a code. */
export function parseCustomTheme(code: string): CustomTheme | null {
  const colors = code.split(/[\s,]+/).filter(Boolean).map(normalizeHex)
  const [sidebar, selected, accent, chat] = colors
  if (colors.length < 3 || colors.length > 4 || !sidebar || !selected || !accent || (colors.length === 4 && !chat)) return null
  return { sidebar, selected, accent, chat: chat ?? (readableOn(sidebar) === LIGHT_TEXT ? '#1a1d21' : '#ffffff') }
}

/** `dark` or `light`: the app theme that the message colour reads as. */
export function customThemeScheme(theme: CustomTheme): Scheme {
  return readableOn(theme.chat) === LIGHT_TEXT ? 'dark' : 'light'
}

/** `color-mix(in oklch, text P%, surface)`, as the web writes it. */
const tintOf = (text: string, surface: string) => (percent: number) => mix(text, percent / 100, surface)

/** The surface tokens for one background colour: text, quiet text, hover, borders and fields. */
function surfaceColors(surface: string, accent: string): Partial<Colors> {
  const text = readableOn(surface)
  const tint = tintOf(text, surface)
  return {
    background: surface,
    foreground: text,
    card: tint(4),
    cardForeground: text,
    muted: tint(11),
    mutedForeground: tint(66),
    secondary: tint(13),
    secondaryForeground: text,
    accent: tint(11),
    accentForeground: text,
    border: alpha(text, 0.14),
    input: alpha(text, 0.2),
    ring: tint(50),
    primary: accent,
    primaryForeground: readableOn(accent),
  }
}

/** The page colours: every screen, sheet and menu (the web's `customThemeVariables`). */
export function customThemeColors(theme: CustomTheme): Partial<Colors> {
  const text = readableOn(theme.chat)
  return {
    ...surfaceColors(theme.chat, accentOn(theme.chat, theme.accent)),
    popover: tintOf(text, theme.chat)(6),
    popoverForeground: text,
  }
}

/** The sidebar (drawer) colours, and the scheme its `dark:` styles follow (the web's `appSidebarTheme`). */
export function customSidebarTheme(theme: CustomTheme): { scheme: Scheme; colors: Partial<Colors> } {
  const text = readableOn(theme.sidebar)
  const tint = tintOf(text, theme.sidebar)
  const accent = accentOn(theme.sidebar, theme.accent)
  const popover = mix(accent, 0.08, tint(8))
  return {
    scheme: text === LIGHT_TEXT ? 'dark' : 'light',
    colors: {
      ...surfaceColors(theme.sidebar, theme.accent),
      sidebar: theme.sidebar,
      sidebarWash: 'rgba(0, 0, 0, 0)',
      sidebarForeground: text,
      sidebarAccent: theme.selected,
      sidebarAccentForeground: readableOn(theme.selected),
      sidebarBorder: alpha(text, 0.14),
      sidebarEdge: alpha(text, 0.1),
      sidebarNav: tint(72),
      sidebarNavActive: text,
      sidebarNavMuted: tint(55),
      sidebarTint: accent,
      sidebarTintForeground: mix(accent, 0.55, text),
      sidebarPrimary: accent,
      sidebarPrimaryForeground: readableOn(accent),
      sidebarPopover: popover,
      popover,
      popoverForeground: text,
    },
  }
}

/** Uniwind's variable for a colour token: `sidebarNavActive` → `--color-sidebar-nav-active`. */
export function colorVariables(colors: Partial<Colors>): Record<string, string> {
  return Object.fromEntries(
    (Object.entries(colors) as [ColorName, string][]).map(([name, value]) => [`--color-${name.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`, value]),
  )
}
