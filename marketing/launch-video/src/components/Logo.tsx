import { interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion'
import { color, display } from '../theme'

// The pixel "O" from apps/web/public/logo.svg: four bars on an 11x11 grid, each with a shadow 1 unit down-right.
const BARS = [
  { x: 2, y: 0, w: 6, h: 2 },
  { x: 8, y: 2, w: 2, h: 6 },
  { x: 2, y: 8, w: 6, h: 2 },
  { x: 0, y: 2, w: 2, h: 6 },
]

/** Orbit mark: the pixel "O". Its bars drop into place one by one, clockwise from the top. */
export function OrbitMark({ size = 160, from = 0 }: { size?: number; from?: number }) {
  const frame = useCurrentFrame() - from
  const { fps } = useVideoConfig()
  const pops = BARS.map((_, index) => spring({ frame: frame - index * 4, fps, config: { damping: 12, stiffness: 160 } }))
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 11 11"
      shapeRendering="crispEdges"
      style={{ overflow: 'visible', filter: 'drop-shadow(0 0 18px rgba(242,69,143,0.45))' }}
    >
      {BARS.map((bar, index) => (
        <rect key={index} x={bar.x + 1} y={bar.y + 1} width={bar.w} height={bar.h} fill={color.pinkShadow} opacity={Math.min(pops[index], 1)} />
      ))}
      {BARS.map((bar, index) => (
        <rect
          key={index}
          x={bar.x}
          y={bar.y - (1 - pops[index]) * 2}
          width={bar.w}
          height={bar.h}
          fill={color.pink}
          opacity={Math.min(pops[index], 1)}
        />
      ))}
    </svg>
  )
}

export function Wordmark({ size = 120, from = 0 }: { size?: number; from?: number }) {
  const frame = useCurrentFrame() - from
  const { fps } = useVideoConfig()
  const p = spring({ frame, fps, config: { damping: 200 }, durationInFrames: 26 })
  return (
    <div
      style={{
        fontFamily: display,
        fontSize: size,
        fontWeight: 700,
        letterSpacing: '-0.055em',
        color: color.text,
        clipPath: `inset(0 ${100 - p * 100}% 0 0)`,
        transform: `translateX(${interpolate(p, [0, 1], [-20, 0])}px)`,
      }}
    >
      Orbit
    </div>
  )
}
