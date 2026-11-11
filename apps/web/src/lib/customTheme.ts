import { useSyncExternalStore, type CSSProperties } from 'react'

/**
 * Account settings → Appearance → Custom: a personal colour theme for the whole app, in place of Light, System or
 * Dark. Four colours, and every text colour is derived so that it stays readable. `null` is the chosen app theme.
 */
export interface CustomTheme {
  /** The sidebar background. */
  sidebar: string
  /** The open conversation's row. */
  selected: string
  /** Unread markers, count badges, mentions and the send button. */
  accent: string
  /** The background of every page: messages, headers, panes, menus and dialogs. */
  chat: string
}

export interface CustomThemePreset {
  name: string
  theme: CustomTheme
}

const NOTION: CustomThemePreset = { name: 'Notion', theme: { sidebar: '#f7f7f5', selected: '#ebebe9', accent: '#2383e2', chat: '#ffffff' } }
const SLACK: CustomThemePreset = { name: 'Slack', theme: { sidebar: '#3f0e40', selected: '#1164a3', accent: '#ecb22e', chat: '#ffffff' } }
const LINEAR: CustomThemePreset = { name: 'Linear', theme: { sidebar: '#0e0f11', selected: '#24252a', accent: '#5e6ad2', chat: '#16171a' } }

export const CUSTOM_THEME_PRESETS: readonly CustomThemePreset[] = [
  SLACK,
  NOTION,
  LINEAR,
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

/** The part of the app that a page belongs to, for Appearance → Per app. */
export type AppSection = 'docs' | 'chat' | 'tasks'
export const APP_SECTIONS: readonly AppSection[] = ['docs', 'chat', 'tasks']

/** Appearance → Per app: the colours of each part of the app. `null` is the chosen Light, System or Dark theme. */
export type AppLooks = Readonly<Record<AppSection, CustomTheme | null>>

/** Per app starts with each part in the colours of the app it replaces. */
export const DEFAULT_APP_LOOKS: AppLooks = { docs: NOTION.theme, chat: SLACK.theme, tasks: LINEAR.theme }

/** The saved choice: colours for the whole app, colours per part of the app, or `null` for Light, System or Dark. */
export type CustomThemeChoice = CustomTheme | { apps: AppLooks } | null

/** A theme's preset name, `Your colors` when it is not a preset, or `Orbit` for the Light, System or Dark theme. */
export function themeName(theme: CustomTheme | null): string {
  if (!theme) return 'Orbit'
  const code = serializeCustomTheme(theme)
  return CUSTOM_THEME_PRESETS.find((preset) => serializeCustomTheme(preset.theme) === code)?.name ?? 'Your colors'
}

export function appSectionOf(pathname: string): AppSection | null {
  if (/^\/docs(\/|$)/.test(pathname)) return 'docs'
  if (/^\/chat(\/|$)/.test(pathname)) return 'chat'
  if (/^\/(tasks|views)(\/|$)/.test(pathname)) return 'tasks'
  return null
}

// the old name, from when the theme coloured only chat: renaming it would drop saved themes
const STORAGE_KEY = 'orbit:chat:theme'
// Per app: `apps:` and a JSON object of share codes, for example `apps:{"docs":"#f7f7f5,…","chat":null,"tasks":…}`
const APPS_PREFIX = 'apps:'
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
export function serializeCustomTheme(theme: CustomTheme): string {
  return [theme.sidebar, theme.selected, theme.accent, theme.chat].join(',')
}

/**
 * Reads a shared code: four hex colours separated by commas or spaces. Three colours are a sidebar-only code; the
 * message surface is then a neutral one that agrees with the sidebar. `null` when the text is not a code.
 */
export function parseCustomTheme(code: string): CustomTheme | null {
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


/** `dark` or `light`: the app theme that the custom theme's message colour reads as. */
export function customThemeScheme(theme: CustomTheme): 'dark' | 'light' {
  return readableOn(theme.chat) === LIGHT_TEXT ? 'dark' : 'light'
}

/**
 * The shadcn token overrides for the page surface (`bg-background`, `text-foreground`, `bg-muted`, `bg-primary`…).
 * `ThemeProvider` puts them on the document root, so every page and portalled menu or dialog gets them.
 */
export function customThemeVariables(theme: CustomTheme | null): CSSProperties {
  if (!theme) return {}
  const text = readableOn(theme.chat)
  return {
    ...surfaceVariables(theme.chat, accentOn(theme.chat, theme.accent)),
    '--popover': `color-mix(in oklch, ${text} 6%, ${theme.chat})`,
    '--popover-foreground': text,
  } as CSSProperties
}

/** The overrides for the sidebar inside the chat page: its own surface, and the colour of the open row. */
export function customSidebarVariables(theme: CustomTheme | null): CSSProperties {
  if (!theme) return {}
  return {
    ...surfaceVariables(theme.sidebar, theme.accent),
    '--chat-selected': theme.selected,
    '--chat-selected-foreground': readableOn(theme.selected),
  } as CSSProperties
}

const listeners = new Set<() => void>()
let cached: { raw: string | null; theme: CustomThemeChoice } | null = null
let section: AppSection | null = null

function read(): CustomThemeChoice {
  let raw: string | null = null
  try {
    raw = globalThis.localStorage?.getItem(STORAGE_KEY) ?? null
  } catch {
    // Unreadable storage: the default theme.
  }
  if (!cached || cached.raw !== raw) cached = { raw, theme: raw ? parseChoice(raw) : null }
  return cached.theme
}

function parseChoice(raw: string): CustomThemeChoice {
  if (!raw.startsWith(APPS_PREFIX)) return parseCustomTheme(raw)
  let codes: unknown
  try {
    codes = JSON.parse(raw.slice(APPS_PREFIX.length))
  } catch {
    return { apps: DEFAULT_APP_LOOKS }
  }
  const code = (section: AppSection) => (codes && typeof codes === 'object' ? (codes as Record<string, unknown>)[section] : undefined)
  const look = (section: AppSection) => {
    const value = code(section)
    return typeof value === 'string' ? parseCustomTheme(value) : value === null ? null : DEFAULT_APP_LOOKS[section]
  }
  return { apps: { docs: look('docs'), chat: look('chat'), tasks: look('tasks') } }
}

function serializeChoice(choice: NonNullable<CustomThemeChoice>): string {
  if (!('apps' in choice)) return serializeCustomTheme(choice)
  const codes = Object.fromEntries(APP_SECTIONS.map((section) => [section, choice.apps[section] && serializeCustomTheme(choice.apps[section])]))
  return APPS_PREFIX + JSON.stringify(codes)
}

/** The theme on screen: your own colours, or under Per app the colours of the open page's part of the app. */
function readActive(): CustomTheme | null {
  const choice = read()
  if (!choice || !('apps' in choice)) return choice
  return section ? choice.apps[section] : null
}

/** Per app: new colours for one part of the app, and Per app on when it was not. */
export function setAppLook(section: AppSection, theme: CustomTheme | null) {
  const choice = read()
  const looks = choice && 'apps' in choice ? choice.apps : DEFAULT_APP_LOOKS
  setCustomTheme({ apps: { ...looks, [section]: theme } })
}

/** Saves the theme in this browser (theme sync is a backend project). `null` goes back to the Orbit default. */
export function setCustomTheme(theme: CustomThemeChoice) {
  try {
    if (theme) globalThis.localStorage?.setItem(STORAGE_KEY, serializeChoice(theme))
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

/** `AppShell` tells which page is open, so that Per app can colour it. `null` when no app page is open. */
export function setOpenPage(pathname: string | null) {
  const next = pathname === null ? null : appSectionOf(pathname)
  if (next === section) return
  section = next
  for (const listener of listeners) listener()
}

/** The saved choice, for Appearance: it shows which card is on, whatever page is open. */
export function useCustomThemeChoice(): CustomThemeChoice {
  return useSyncExternalStore(subscribe, read, () => null)
}

/** The colours on screen now, or `null` for the chosen Light, System or Dark theme. */
export function useCustomTheme(): CustomTheme | null {
  return useSyncExternalStore(subscribe, readActive, () => null)
}

/**
 * The custom theme on the app sidebar: its surface, the nav row colours and the open chat row. `scheme` is the class
 * for the sidebar (`dark` or `light`), so its `dark:` styles follow the theme's sidebar colour and not the app theme.
 */
export function appSidebarTheme(theme: CustomTheme | null): { scheme?: 'dark' | 'light'; style?: CSSProperties } {
  if (!theme) return {}
  const text = readableOn(theme.sidebar)
  const tint = (percent: number) => `color-mix(in oklch, ${text} ${percent}%, ${theme.sidebar})`
  const accent = accentOn(theme.sidebar, theme.accent)
  return {
    scheme: text === LIGHT_TEXT ? 'dark' : 'light',
    style: {
      ...customSidebarVariables(theme),
      '--sidebar': theme.sidebar,
      '--sidebar-wash': 'transparent',
      '--sidebar-foreground': text,
      // the selected rows (docs tree, chat list, project rail) use the theme's selected colour
      '--sidebar-accent': theme.selected,
      '--sidebar-accent-foreground': readableOn(theme.selected),
      '--sidebar-border': `color-mix(in oklch, ${text} 14%, transparent)`,
      '--sidebar-edge': `color-mix(in oklch, ${text} 10%, transparent)`,
      '--sidebar-nav': tint(72),
      '--sidebar-nav-active': text,
      '--sidebar-nav-muted': tint(55),
      '--sidebar-tint': accent,
      '--sidebar-tint-foreground': `color-mix(in oklch, ${accent} 55%, ${text})`,
      '--sidebar-primary': accent,
      '--sidebar-primary-foreground': readableOn(accent),
      // the menus opened from the sidebar (workspace, account): a raised surface with a hint of the accent
      '--sidebar-popover': `color-mix(in oklch, ${accent} 8%, ${tint(8)})`,
      '--popover': 'var(--sidebar-popover)',
      '--popover-foreground': text,
    } as CSSProperties,
  }
}

/** The custom theme for the sidebar and the menus it opens (they are portalled out of it, so they need it too). */
export function useAppSidebarTheme() {
  return appSidebarTheme(useCustomTheme())
}
