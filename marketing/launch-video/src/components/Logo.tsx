import { interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion'
import { color, display } from '../theme'

/** Orbit mark: a planet with a tilted ring that draws itself in, and a moon travelling on the ring. */
export function OrbitMark({ size = 160, from = 0 }: { size?: number; from?: number }) {
  const frame = useCurrentFrame() - from
  const { fps } = useVideoConfig()
  const pop = spring({ frame, fps, config: { damping: 14, stiffness: 120 } })
  const draw = interpolate(frame, [4, 34], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
  const angle = frame * 0.06 + 2.2
  const rx = 46
  const ry = 16
  const mx = 50 + rx * Math.cos(angle)
  const my = 50 + ry * Math.sin(angle)
  const behind = Math.sin(angle) < 0
  const circumference = 2 * Math.PI * Math.sqrt((rx * rx + ry * ry) / 2)
  const moon = <circle cx={mx} cy={my} r={4.2} fill="#fff" opacity={draw} style={{ filter: 'drop-shadow(0 0 4px #fff)' }} />
  return (
    <svg width={size} height={size} viewBox="0 0 100 100" style={{ overflow: 'visible', transform: `scale(${pop})` }}>
      <defs>
        <linearGradient id="planet" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#ff8ac0" />
          <stop offset="55%" stopColor={color.pink} />
          <stop offset="100%" stopColor={color.violet} />
        </linearGradient>
      </defs>
      <g transform="rotate(-18 50 50)">
        {behind ? moon : null}
        <circle cx="50" cy="50" r="24" fill="url(#planet)" style={{ filter: 'drop-shadow(0 0 18px rgba(242,69,143,0.6))' }} />
        <ellipse
          cx="50"
          cy="50"
          rx={rx}
          ry={ry}
          fill="none"
          stroke="rgba(255,255,255,0.85)"
          strokeWidth="3"
          strokeDasharray={circumference}
          strokeDashoffset={circumference * (1 - draw)}
          strokeLinecap="round"
        />
        {behind ? null : moon}
      </g>
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
