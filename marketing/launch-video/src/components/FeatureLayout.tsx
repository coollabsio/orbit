import type { ReactNode } from 'react'
import { AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion'
import { color, display } from '../theme'
import { Background } from './Background'
import { Body, Eyebrow, FadeUp, Reveal } from './Text'

export type Chip = { label: string; dot?: string }

/** Text column on one side, a screenshot window bleeding off the other edge in soft 3D. */
export function FeatureLayout({
  side = 'left',
  eyebrow,
  title,
  body,
  chips = [],
  children,
}: {
  side?: 'left' | 'right'
  eyebrow: string
  title: string
  body: string
  chips?: Chip[]
  children: ReactNode
}) {
  const frame = useCurrentFrame()
  const { fps, durationInFrames } = useVideoConfig()
  const enter = spring({ frame: frame - 4, fps, config: { damping: 22, stiffness: 90, mass: 1.1 } })
  const drift = interpolate(frame, [0, durationInFrames], [0, 1])
  const dir = side === 'left' ? 1 : -1
  const rotateY = interpolate(enter, [0, 1], [-22, -7]) * dir + drift * 2 * dir
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ perspective: 2600 }}>
        <div
          style={{
            position: 'absolute',
            top: '50%',
            [side === 'left' ? 'left' : 'right']: 740,
            transform: `translate(${interpolate(enter, [0, 1], [260, 0]) * dir}px, -50%) rotateY(${rotateY}deg) scale(${interpolate(enter, [0, 1], [0.92, 1])})`,
            transformOrigin: side === 'left' ? 'left center' : 'right center',
            opacity: enter,
          }}
        >
          {children}
        </div>
      </AbsoluteFill>
      <div
        style={{
          position: 'absolute',
          top: 0,
          bottom: 0,
          [side]: 0,
          width: 760,
          padding: side === 'left' ? '0 40px 0 120px' : '0 120px 0 60px',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          gap: 28,
          background: `linear-gradient(${side === 'left' ? 90 : 270}deg, ${color.bg} 55%, transparent)`,
        }}
      >
        <div>
          <Eyebrow from={6}>{eyebrow}</Eyebrow>
        </div>
        <Reveal text={title} from={12} size={74} />
        <Body from={26}>{body}</Body>
        {chips.length > 0 ? (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 12, maxWidth: 580 }}>
            {chips.map((chip, index) => (
              <FadeUp key={chip.label} from={36 + index * 5} distance={14}>
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 10,
                    padding: '10px 18px',
                    borderRadius: 12,
                    background: 'rgba(255,255,255,0.05)',
                    border: `1px solid ${color.faint}`,
                    fontFamily: display,
                    fontSize: 22,
                    fontWeight: 500,
                    color: color.text,
                  }}
                >
                  <span style={{ width: 9, height: 9, borderRadius: 99, background: chip.dot ?? color.pink }} />
                  {chip.label}
                </div>
              </FadeUp>
            ))}
          </div>
        ) : null}
      </div>
    </AbsoluteFill>
  )
}
