import type { CSSProperties, ReactNode } from 'react'
import { interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion'
import { color, display } from '../theme'

/** Word-by-word rise + unblur. Wrap a word in *asterisks* to paint it with the brand gradient. */
export function Reveal({
  text,
  from = 0,
  stagger = 3,
  size = 72,
  weight = 600,
  style,
  align = 'left',
}: {
  text: string
  from?: number
  stagger?: number
  size?: number
  weight?: number
  style?: CSSProperties
  align?: 'left' | 'center'
}) {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const words = text.split(' ')
  return (
    <div
      style={{
        fontFamily: display,
        fontSize: size,
        fontWeight: weight,
        lineHeight: 1.05,
        letterSpacing: '-0.035em',
        color: color.text,
        textAlign: align,
        ...style,
      }}
    >
      {words.map((raw, index) => {
        const accent = raw.startsWith('*') && raw.endsWith('*')
        const word = accent ? raw.slice(1, -1) : raw
        const p = spring({ frame: frame - from - index * stagger, fps, config: { damping: 200 }, durationInFrames: 22 })
        return (
          <span
            key={index}
            style={{
              display: 'inline-block',
              marginRight: '0.24em',
              opacity: p,
              transform: `translateY(${interpolate(p, [0, 1], [0.45, 0])}em)`,
              filter: `blur(${interpolate(p, [0, 1], [10, 0])}px)`,
              ...(accent ? gradientText : null),
            }}
          >
            {word}
          </span>
        )
      })}
    </div>
  )
}

export const gradientText: CSSProperties = {
  backgroundImage: `linear-gradient(100deg, ${color.pink} 10%, #ff8ac0 50%, ${color.violet} 95%)`,
  WebkitBackgroundClip: 'text',
  backgroundClip: 'text',
  color: 'transparent',
}

/** Fades and lifts any block in. */
export function FadeUp({ from = 0, children, distance = 24, style }: { from?: number; children: ReactNode; distance?: number; style?: CSSProperties }) {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const p = spring({ frame: frame - from, fps, config: { damping: 200 }, durationInFrames: 24 })
  return (
    <div style={{ opacity: p, transform: `translateY(${interpolate(p, [0, 1], [distance, 0])}px)`, filter: `blur(${interpolate(p, [0, 1], [6, 0])}px)`, ...style }}>
      {children}
    </div>
  )
}

export function Eyebrow({ children, from = 0 }: { children: ReactNode; from?: number }) {
  return (
    <FadeUp from={from}>
      <div
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 12,
          padding: '8px 16px 8px 12px',
          borderRadius: 999,
          border: '1px solid rgba(242,69,143,0.35)',
          background: 'rgba(242,69,143,0.08)',
          color: '#ff9cc8',
          fontFamily: display,
          fontSize: 22,
          fontWeight: 500,
          letterSpacing: '0.01em',
        }}
      >
        <span style={{ width: 8, height: 8, borderRadius: 99, background: color.pink, boxShadow: `0 0 12px ${color.pink}` }} />
        {children}
      </div>
    </FadeUp>
  )
}

export function Body({ children, from = 0, width = 560 }: { children: ReactNode; from?: number; width?: number }) {
  return (
    <FadeUp from={from}>
      <p style={{ margin: 0, maxWidth: width, fontFamily: display, fontSize: 28, lineHeight: 1.45, color: color.muted, letterSpacing: '-0.01em' }}>{children}</p>
    </FadeUp>
  )
}
