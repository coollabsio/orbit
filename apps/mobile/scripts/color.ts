/** Converts the CSS color syntax used in apps/web/src/index.css to hex or rgba() for React Native. */

export interface Rgba {
  /** Gamma-encoded sRGB channels, 0..1, not yet clamped. */
  r: number
  g: number
  b: number
  alpha: number
}

type Lab = [number, number, number]

const srgbToLinear = (x: number) => (Math.abs(x) <= 0.04045 ? x / 12.92 : Math.sign(x) * ((Math.abs(x) + 0.055) / 1.055) ** 2.4)
const linearToSrgb = (x: number) => (Math.abs(x) <= 0.0031308 ? 12.92 * x : Math.sign(x) * (1.055 * Math.abs(x) ** (1 / 2.4) - 0.055))

export function oklabToRgb([L, a, b]: Lab): [number, number, number] {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  return [
    linearToSrgb(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    linearToSrgb(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    linearToSrgb(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
  ]
}

export function rgbToOklab(r: number, g: number, b: number): Lab {
  const [lr, lg, lb] = [r, g, b].map(srgbToLinear) as [number, number, number]
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb)
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb)
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ]
}

const number = (token: string, percentScale = 1) => (token.endsWith('%') ? (Number.parseFloat(token) / 100) * percentScale : Number.parseFloat(token))

function parseAlpha(token: string | undefined) {
  return token === undefined ? 1 : number(token)
}

/** Splits `a b c / d` (space syntax) into channel tokens and the optional alpha token. */
function channels(body: string): { parts: string[]; alpha?: string } {
  const [main, alpha] = body.split('/').map((part) => part.trim())
  return { parts: main.split(/[\s,]+/).filter(Boolean), alpha }
}

/** Splits on top-level commas, ignoring commas inside nested parentheses. */
function topLevelArgs(body: string): string[] {
  const args: string[] = []
  let depth = 0
  let current = ''
  for (const char of body) {
    if (char === '(') depth++
    if (char === ')') depth--
    if (char === ',' && depth === 0) {
      args.push(current.trim())
      current = ''
    } else current += char
  }
  args.push(current.trim())
  return args
}

export type Resolve = (name: string) => string

/** Parses one CSS color. `resolve` returns the raw value of a `var(--name)` reference. */
export function parseColor(input: string, resolve: Resolve = (name) => { throw new Error(`Unresolved var(--${name})`) }): Rgba {
  const value = input.trim()
  const fn = /^([a-z-]+)\((.*)\)$/is.exec(value)
  if (value === 'transparent') return { r: 0, g: 0, b: 0, alpha: 0 }
  if (value.startsWith('#')) {
    const hex = value.slice(1)
    const full = hex.length <= 4 ? [...hex].map((c) => c + c).join('') : hex
    const byte = (i: number) => Number.parseInt(full.slice(i, i + 2), 16) / 255
    return { r: byte(0), g: byte(2), b: byte(4), alpha: full.length === 8 ? byte(6) : 1 }
  }
  if (!fn) throw new Error(`Unsupported color: ${value}`)
  const [, name, body] = fn
  switch (name.toLowerCase()) {
    case 'var':
      return parseColor(resolve(body.trim().replace(/^--/, '')), resolve)
    case 'oklch': {
      const { parts, alpha } = channels(body)
      const L = number(parts[0])
      const C = number(parts[1], 0.4)
      const h = (Number.parseFloat(parts[2]) * Math.PI) / 180
      const [r, g, b] = oklabToRgb([L, C * Math.cos(h), C * Math.sin(h)])
      return { r, g, b, alpha: parseAlpha(alpha) }
    }
    case 'oklab': {
      const { parts, alpha } = channels(body)
      const [r, g, b] = oklabToRgb([number(parts[0]), number(parts[1], 0.4), number(parts[2], 0.4)])
      return { r, g, b, alpha: parseAlpha(alpha) }
    }
    case 'rgb':
    case 'rgba': {
      const { parts, alpha } = channels(body)
      const [r, g, b] = parts.slice(0, 3).map((part) => number(part, 255) / 255)
      return { r, g, b, alpha: parseAlpha(alpha ?? parts[3]) }
    }
    case 'color-mix':
      return colorMix(body, resolve)
    default:
      throw new Error(`Unsupported color function: ${name}`)
  }
}

/** `color-mix(in oklab, A p%, B q%)` per CSS Color 5: premultiplied interpolation in OKLab. */
function colorMix(body: string, resolve: Resolve): Rgba {
  const [space, ...stops] = topLevelArgs(body)
  if (space.trim() !== 'in oklab') throw new Error(`Unsupported color-mix space: ${space}`)
  const parsed = stops.map((stop) => {
    const match = /^(.*?)(?:\s+([\d.]+%))?$/s.exec(stop.trim())!
    return { color: parseColor(match[1], resolve), percent: match[2] ? Number.parseFloat(match[2]) / 100 : undefined }
  })
  const [first, second] = parsed
  const p1 = first.percent ?? (second.percent === undefined ? 0.5 : 1 - second.percent)
  const p2 = second.percent ?? 1 - p1
  const total = p1 + p2
  const [w1, w2] = [p1 / total, p2 / total]
  const lab1 = rgbToOklab(first.color.r, first.color.g, first.color.b)
  const lab2 = rgbToOklab(second.color.r, second.color.g, second.color.b)
  const alpha = first.color.alpha * w1 + second.color.alpha * w2
  const mixed = [0, 1, 2].map((i) => (lab1[i] * first.color.alpha * w1 + lab2[i] * second.color.alpha * w2) / (alpha || 1)) as Lab
  const [r, g, b] = oklabToRgb(mixed)
  return { r, g, b, alpha: alpha * Math.min(total, 1) }
}

const byte = (channel: number) => Math.round(Math.min(1, Math.max(0, channel)) * 255)

/** `#rrggbb` when opaque, `rgba(r, g, b, a)` otherwise. Channels are clamped to the sRGB gamut. */
export function formatColor({ r, g, b, alpha }: Rgba): string {
  if (alpha >= 1) return `#${[r, g, b].map((c) => byte(c).toString(16).padStart(2, '0')).join('')}`
  return `rgba(${byte(r)}, ${byte(g)}, ${byte(b)}, ${Number(alpha.toFixed(3))})`
}

export const cssColorToRn = (input: string, resolve?: Resolve) => formatColor(parseColor(input, resolve))
