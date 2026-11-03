/**
 * The CSS color math the web relies on (`color-mix(in oklch, …)`, `/<alpha>` tints), so the app derives the same
 * colors from the theme tokens instead of hard-coding them. No React.
 */

/** sRGB channels and alpha, each 0..1. */
interface Rgba {
  r: number
  g: number
  b: number
  a: number
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const k = (n: number) => (n + h / 30) % 12
  const f = (n: number) => l - s * Math.min(l, 1 - l) * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1))
  return [f(0), f(8), f(4)]
}

/** `#rgb`, `#rrggbb`, `rgb(…)`, `rgba(…)` and `hsl(h s% l%)`, the formats the tokens and `userColor` produce. */
function parse(color: string): Rgba {
  const value = color.trim()
  if (value.startsWith('#')) {
    const hex = value.length === 4 ? [...value.slice(1)].map((digit) => digit + digit).join('') : value.slice(1, 7)
    return { r: parseInt(hex.slice(0, 2), 16) / 255, g: parseInt(hex.slice(2, 4), 16) / 255, b: parseInt(hex.slice(4, 6), 16) / 255, a: 1 }
  }
  const numbers = (value.match(/-?[\d.]+/g) ?? []).map(Number)
  if (value.startsWith('hsl')) {
    const [r, g, b] = hslToRgb(numbers[0] ?? 0, (numbers[1] ?? 0) / 100, (numbers[2] ?? 0) / 100)
    return { r, g, b, a: numbers[3] ?? 1 }
  }
  if (value.startsWith('rgb')) return { r: (numbers[0] ?? 0) / 255, g: (numbers[1] ?? 0) / 255, b: (numbers[2] ?? 0) / 255, a: numbers[3] ?? 1 }
  throw new Error(`Unsupported color: ${color}`)
}

function format({ r, g, b, a }: Rgba): string {
  const channel = (value: number) => Math.round(Math.min(1, Math.max(0, value)) * 255)
  if (a >= 1) return `#${[r, g, b].map((value) => channel(value).toString(16).padStart(2, '0')).join('')}`
  return `rgba(${channel(r)}, ${channel(g)}, ${channel(b)}, ${Math.round(a * 1000) / 1000})`
}

/** The color at `amount` of its opacity: Tailwind's `text-muted-foreground/70` is `alpha(mutedForeground, 0.7)`. */
export function alpha(color: string, amount: number): string {
  const rgba = parse(color)
  return format({ ...rgba, a: rgba.a * amount })
}

const toLinear = (value: number) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4)
const toGamma = (value: number) => (value <= 0.0031308 ? 12.92 * value : 1.055 * value ** (1 / 2.4) - 0.055)

/** [L, C, h in degrees]. */
type Oklch = [number, number, number]

function toOklch({ r, g, b }: Rgba): Oklch {
  const [lr, lg, lb] = [toLinear(r), toLinear(g), toLinear(b)]
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb)
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb)
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb)
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
  const hue = (Math.atan2(B, A) * 180) / Math.PI
  return [L, Math.hypot(A, B), hue < 0 ? hue + 360 : hue]
}

function fromOklch([L, C, h]: Oklch, a: number): Rgba {
  const A = C * Math.cos((h * Math.PI) / 180)
  const B = C * Math.sin((h * Math.PI) / 180)
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3
  return {
    r: toGamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    g: toGamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    b: toGamma(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
    a,
  }
}

/**
 * The web authors its grays as `oklch(L 0 0)`: an explicit hue of 0, which a mix interpolates toward (a hue the browser
 * would treat as missing for a hex gray). The tokens arrive here as hex, so a gray's hue is set back to 0.
 */
const ACHROMATIC = 2e-4

/** CSS `color-mix(in oklch, <first> <weight*100>%, <second>)` with the shorter hue arc. */
/**
 * A color as an opaque color plus its opacity. react-native-svg on iOS ignores the alpha of an `rgba()` `stopColor`,
 * so a gradient stop needs them apart (`stopColor` + `stopOpacity`).
 */
export function splitAlpha(color: string): { color: string; opacity: number } {
  const rgba = parse(color)
  return { color: format({ ...rgba, a: 1 }), opacity: rgba.a }
}

export function mix(first: string, weight: number, second: string): string {
  const one = parse(first)
  const two = parse(second)
  const [L1, C1, h1] = toOklch(one)
  const [L2, C2, h2] = toOklch(two)
  const hueOne = C1 < ACHROMATIC ? 0 : h1
  const hueTwo = C2 < ACHROMATIC ? 0 : h2
  let delta = hueTwo - hueOne
  if (delta > 180) delta -= 360
  if (delta < -180) delta += 360
  const p = 1 - weight
  const hue = (hueOne + delta * p + 360) % 360
  return format(fromOklch([L1 + (L2 - L1) * p, C1 + (C2 - C1) * p, hue], one.a + (two.a - one.a) * p))
}

/**
 * A person's color, as the web derives it from the user id (`userColor` in `features/workspaces/api.ts`). Kept as
 * `hsl()` so mixes start from the exact color, as the browser's do.
 */
export function userColor(userId: string): string {
  const hue = [...userId].reduce((total, character) => total + character.charCodeAt(0), 0) % 360
  return `hsl(${hue}, 55%, 48%)`
}

/** The web `UserAvatar` fallback: a tint of the person's color on the muted surface, initials toward the text color. */
export function avatarColors(color: string, muted: string, foreground: string): { background: string; text: string } {
  return { background: mix(color, 0.26, muted), text: mix(color, 0.6, foreground) }
}
