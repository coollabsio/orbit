import { interpolate, useCurrentFrame } from 'remotion'
import { color, ease } from '../theme'
import { useShotScale } from './AppWindow'

export type CursorKey = { f: number; x: number; y: number }

/** A macOS-style pointer moving through shot coordinates, with click ripples at `clicks` frames. */
export function Cursor({ path, clicks = [], from = 0 }: { path: CursorKey[]; clicks?: number[]; from?: number }) {
  const frame = useCurrentFrame()
  const scale = useShotScale()
  if (frame < from) return null
  let x = path[0].x
  let y = path[0].y
  for (let i = 1; i < path.length; i++) {
    const a = path[i - 1]
    const b = path[i]
    if (frame >= a.f) {
      const t = ease(Math.min(1, Math.max(0, (frame - a.f) / (b.f - a.f))))
      x = a.x + (b.x - a.x) * t
      y = a.y + (b.y - a.y) * t
    }
  }
  const pressed = clicks.some((c) => frame >= c && frame < c + 6)
  const opacity = interpolate(frame, [from, from + 8], [0, 1], { extrapolateRight: 'clamp' })
  return (
    <div style={{ position: 'absolute', left: x, top: y, transform: `scale(${1 / scale})`, transformOrigin: '0 0', opacity, zIndex: 10 }}>
      {clicks.map((c) => {
        const t = frame - c
        if (t < 0 || t > 18) return null
        const r = interpolate(t, [0, 18], [6, 44])
        return (
          <span
            key={c}
            style={{
              position: 'absolute',
              left: -r,
              top: -r,
              width: r * 2,
              height: r * 2,
              borderRadius: 999,
              border: `2px solid ${color.pink}`,
              opacity: interpolate(t, [0, 18], [0.9, 0]),
            }}
          />
        )
      })}
      <svg width="30" height="30" viewBox="0 0 24 24" style={{ transform: `scale(${pressed ? 0.85 : 1})`, filter: 'drop-shadow(0 4px 8px rgba(0,0,0,0.6))' }}>
        <path d="M4 2.5 L4 19 L8.6 14.9 L11.6 21.6 L14.6 20.3 L11.7 13.7 L17.8 13.4 Z" fill="#fff" stroke="#000" strokeWidth="1.3" strokeLinejoin="round" />
      </svg>
    </div>
  )
}
