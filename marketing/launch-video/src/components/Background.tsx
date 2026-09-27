import { AbsoluteFill, useCurrentFrame } from 'remotion'
import { color } from '../theme'

/** Slow-moving brand glows over a faint, drifting grid. */
export function Background({ intensity = 1, grid = true }: { intensity?: number; grid?: boolean }) {
  const frame = useCurrentFrame()
  const t = frame / 30
  const ax = 22 + Math.sin(t * 0.35) * 8
  const ay = 18 + Math.cos(t * 0.3) * 6
  const bx = 78 + Math.cos(t * 0.28) * 7
  const by = 82 + Math.sin(t * 0.33) * 6
  return (
    <AbsoluteFill style={{ backgroundColor: color.bg, overflow: 'hidden' }}>
      <AbsoluteFill
        style={{
          opacity: intensity,
          background: [
            `radial-gradient(900px 600px at ${ax}% ${ay}%, rgba(242,69,143,0.22), transparent 70%)`,
            `radial-gradient(1000px 700px at ${bx}% ${by}%, rgba(139,92,246,0.18), transparent 70%)`,
          ].join(','),
        }}
      />
      {grid ? (
        <AbsoluteFill
          style={{
            backgroundImage:
              'linear-gradient(rgba(255,255,255,0.035) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.035) 1px, transparent 1px)',
            backgroundSize: '64px 64px',
            backgroundPosition: `${(frame * 0.25) % 64}px ${(frame * 0.15) % 64}px`,
            maskImage: 'radial-gradient(ellipse 70% 60% at 50% 50%, black 30%, transparent 80%)',
          }}
        />
      ) : null}
      <AbsoluteFill style={{ background: 'radial-gradient(ellipse at center, transparent 55%, rgba(0,0,0,0.55) 100%)' }} />
    </AbsoluteFill>
  )
}
