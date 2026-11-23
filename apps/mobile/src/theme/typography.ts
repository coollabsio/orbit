import type { TextStyle } from 'react-native'

/** Settings → Appearance → Font, as on the web. `noto-sans` is the default; `system` is the device's UI font. */
export const FONTS = ['noto-sans', 'geist', 'inter', 'ibm-plex-sans', 'system', 'literata', 'mono'] as const
export type FontChoice = (typeof FONTS)[number]

/** Settings → Appearance → Text size, smallest first, with the web's root font size for each. */
export const TEXT_SIZES = ['small', 'compact', 'default', 'large', 'extra-large'] as const
export type TextSize = (typeof TEXT_SIZES)[number]
export const TEXT_SCALE: Record<TextSize, number> = { small: 0.875, compact: 0.9375, default: 1, large: 1.125, 'extra-large': 1.25 }

/** The four faces the app draws text in: the suffix of every `@expo-google-fonts` family name. */
const WEIGHTS = { '400Regular': '400', '500Medium': '500', '600SemiBold': '600', '700Bold': '700' } as const
export type Face = keyof typeof WEIGHTS

const FAMILY_PREFIX: Record<Exclude<FontChoice, 'system'>, string> = {
  'noto-sans': 'NotoSans',
  geist: 'Geist',
  inter: 'Inter',
  'ibm-plex-sans': 'IBMPlexSans',
  literata: 'Literata',
  mono: 'GeistMono',
}

const NOTO_FACE = /^NotoSans_(400Regular|500Medium|600SemiBold|700Bold)$/

/** The family (and for the system font, the weight) that draws one face of the chosen font. */
export function fontFace(choice: FontChoice, face: Face): { fontFamily: string; fontWeight?: (typeof WEIGHTS)[Face] } {
  // `System` is San Francisco on iOS; Android falls back to its default sans-serif for it
  if (choice === 'system') return { fontFamily: 'System', fontWeight: WEIGHTS[face] }
  return { fontFamily: `${FAMILY_PREFIX[choice]}_${face}` }
}

/**
 * The style that puts a text in the chosen font and size: Noto Sans faces become the chosen font's, and the font size
 * and line height grow or shrink by `scale`. Other fonts (Menlo for code) keep their family. Only changed keys are set.
 */
export function typographyStyle(style: TextStyle, choice: FontChoice, scale: number): TextStyle {
  const fontSize = pixels(style.fontSize)
  const lineHeight = pixels(style.lineHeight)
  const result: TextStyle = {
    ...(scale !== 1 && fontSize !== null ? { fontSize: fontSize * scale } : {}),
    ...(scale !== 1 && lineHeight !== null ? { lineHeight: lineHeight * scale } : {}),
  }
  const face = choice === 'noto-sans' ? null : NOTO_FACE.exec(style.fontFamily ?? '')?.[1]
  return face ? { ...result, ...fontFace(choice, face as Face) } : result
}

/** A size in points: a number on iOS and Android, a CSS `14px` string from Uniwind in the web preview. */
function pixels(value: unknown): number | null {
  if (typeof value === 'number') return value
  const match = typeof value === 'string' ? /^(\d+(?:\.\d+)?)px$/.exec(value) : null
  return match ? Number(match[1]) : null
}
