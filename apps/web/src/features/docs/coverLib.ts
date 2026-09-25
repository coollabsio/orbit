// Cover focal-point math (the reference app's page banner): the position is "x,y" in percent,
// applied as CSS object-position. Dragging pans only the overflow left by object-fit: cover.
export interface CoverPos {
  x: number
  y: number
}

export interface Size {
  w: number
  h: number
}

export const CENTERED_COVER: CoverPos = { x: 50, y: 50 }

const clampPct = (n: number): number => (n < 0 ? 0 : n > 100 ? 100 : n)

export function parseCoverPos(raw: string | null | undefined): CoverPos {
  if (!raw) return CENTERED_COVER
  const [rx, ry] = raw.split(',')
  const x = Number(rx)
  const y = Number(ry)
  if (!Number.isFinite(x) || !Number.isFinite(y)) return CENTERED_COVER
  return { x: clampPct(x), y: clampPct(y) }
}

export function formatCoverPos(pos: CoverPos): string {
  const r = (n: number) => Math.round(clampPct(n) * 10) / 10
  return `${r(pos.x)},${r(pos.y)}`
}

export function coverObjectPosition(pos: CoverPos): string {
  return `${clampPct(pos.x)}% ${clampPct(pos.y)}%`
}

/** Pannable overflow, in px per axis, of an image rendered `object-fit: cover` in `box`. */
export function coverSlack(natural: Size, box: Size): Size {
  if (natural.w <= 0 || natural.h <= 0 || box.w <= 0 || box.h <= 0) return { w: 0, h: 0 }
  const scale = Math.max(box.w / natural.w, box.h / natural.h)
  return { w: Math.max(0, natural.w * scale - box.w), h: Math.max(0, natural.h * scale - box.h) }
}

/** Dragging right (dx > 0) reveals the left part, so the focal point decreases. */
export function dragCoverPos(start: CoverPos, dx: number, dy: number, slack: Size): CoverPos {
  return {
    x: slack.w > 0 ? clampPct(start.x - (dx * 100) / slack.w) : clampPct(start.x),
    y: slack.h > 0 ? clampPct(start.y - (dy * 100) / slack.h) : clampPct(start.y),
  }
}

export function nudgeCoverPos(pos: CoverPos, dx: number, dy: number): CoverPos {
  return { x: clampPct(pos.x + dx), y: clampPct(pos.y + dy) }
}

/** The server only stores http(s) cover URLs (max 2048 chars). */
export function validCoverUrl(raw: string): string | null {
  const url = raw.trim()
  if (url.length === 0 || url.length > 2048) return null
  try {
    const parsed = new URL(url)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:' ? url : null
  } catch {
    return null
  }
}
