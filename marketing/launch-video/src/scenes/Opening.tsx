import { AbsoluteFill, interpolate, Sequence, spring, useCurrentFrame, useVideoConfig } from 'remotion'
import { AppWindow } from '../components/AppWindow'
import { Background } from '../components/Background'
import { OrbitMark, Wordmark } from '../components/Logo'
import { Body, FadeUp, Reveal } from '../components/Text'
import { siLinear, siNotion } from 'simple-icons'
import { color, display, ease, mono, shot } from '../theme'

export function Intro() {
  const frame = useCurrentFrame()
  const { durationInFrames } = useVideoConfig()
  const out = interpolate(frame, [durationInFrames - 14, durationInFrames], [1, 0], { extrapolateLeft: 'clamp' })
  return (
    <AbsoluteFill>
      <Background intensity={interpolate(frame, [0, 30], [0, 1], { extrapolateRight: 'clamp' })} />
      <AbsoluteFill style={{ alignItems: 'center', justifyContent: 'center', gap: 36, opacity: out, transform: `scale(${interpolate(frame, [0, 105], [1, 1.06])})` }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 28 }}>
          <OrbitMark size={150} from={2} />
          <Wordmark size={150} from={18} />
        </div>
        <FadeUp from={40}>
          <div style={{ fontFamily: display, fontSize: 34, color: color.muted, letterSpacing: '-0.01em' }}>Tasks and docs, on your own server.</div>
        </FadeUp>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

const PAINS = ['Tasks in one app.', 'Docs in another.', "Data on someone else's servers."]

/** Three pain points stack up, get struck through, then clear for the reveal. */
export function Hook() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  return (
    <AbsoluteFill>
      <Background intensity={0.5} />
      <AbsoluteFill style={{ alignItems: 'center', justifyContent: 'center', flexDirection: 'column', gap: 18 }}>
        {PAINS.map((pain, index) => {
          const start = 12 + index * 20
          const p = spring({ frame: frame - start, fps, config: { damping: 200 }, durationInFrames: 18 })
          const strike = interpolate(frame, [start + 16, start + 28], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
          return (
            <div
              key={pain}
              style={{
                position: 'relative',
                fontFamily: display,
                fontSize: 104,
                fontWeight: 600,
                letterSpacing: '-0.045em',
                color: color.text,
                opacity: p * interpolate(strike, [0, 1], [1, 0.32]),
                transform: `translateY(${interpolate(p, [0, 1], [40, 0])}px)`,
                filter: `blur(${interpolate(p, [0, 1], [12, 0])}px)`,
              }}
            >
              {pain}
              <span style={{ position: 'absolute', left: -8, right: -8, top: '54%', height: 7, borderRadius: 9, background: color.pink, transformOrigin: 'left', transform: `scaleX(${strike})` }} />
            </div>
          )
        })}
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

/** "Meet Orbit" with the product rising out of perspective. */
export function Hero() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const rise = spring({ frame: frame - 18, fps, config: { damping: 24, stiffness: 70, mass: 1.2 } })
  const behind = spring({ frame: frame - 30, fps, config: { damping: 24, stiffness: 70, mass: 1.2 } })
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', paddingTop: 92, gap: 22 }}>
        <Reveal text="Meet *Orbit.*" size={112} weight={700} align="center" stagger={5} />
        <Body from={14} width={1100}>
          <span style={{ display: 'block', textAlign: 'center' }}>Tasks like Linear. Docs like Notion. One self-hosted workspace.</span>
        </Body>
      </AbsoluteFill>
      <AbsoluteFill style={{ perspective: 2200, alignItems: 'center' }}>
        <div
          style={{
            position: 'absolute',
            top: 330,
            left: 560,
            transform: `translateY(${interpolate(behind, [0, 1], [420, 0])}px) rotateX(${interpolate(behind, [0, 1], [38, 6])}deg) scale(${interpolate(frame, [18, 150], [0.86, 0.92])})`,
            transformOrigin: 'center top',
            opacity: interpolate(behind, [0, 0.4], [0, 0.9], { extrapolateRight: 'clamp' }),
            filter: 'brightness(0.8)',
          }}
        >
          <AppWindow src={shot('docs-page')} width={1300} url="orbit.yourcompany.com/docs/welcome-to-orbit" camera={[{ f: 0, z: 1.05, x: 900, y: 420 }]} />
        </div>
        <div
          style={{
            position: 'absolute',
            top: 380,
            left: 120,
            transform: `translateY(${interpolate(rise, [0, 1], [420, 0])}px) rotateX(${interpolate(rise, [0, 1], [38, 6])}deg) scale(${interpolate(frame, [18, 150], [0.94, 1.02])})`,
            transformOrigin: 'center top',
            opacity: interpolate(rise, [0, 0.4], [0, 1], { extrapolateRight: 'clamp' }),
          }}
        >
          <AppWindow src={shot('list')} width={1180} camera={[{ f: 0, z: 1, x: 800, y: 500 }, { f: 150, z: 1.06, x: 780, y: 360 }]} />
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

function ToolCard({ icon, name, role, x, from }: { icon: { path: string }; name: string; role: string; x: number; from: number }) {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const enter = spring({ frame: frame - from, fps, config: { damping: 18 } })
  // Both cards fly into the centre and collapse into the Orbit mark.
  const merge = interpolate(frame, [58, 78], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: ease })
  return (
    <div
      style={{
        position: 'absolute',
        left: '50%',
        top: 430,
        width: 500,
        marginLeft: -250,
        padding: '44px 46px',
        borderRadius: 26,
        background: 'linear-gradient(180deg, rgba(255,255,255,0.07), rgba(255,255,255,0.02))',
        border: `1px solid ${color.faint}`,
        display: 'flex',
        alignItems: 'center',
        gap: 26,
        opacity: enter * (1 - merge),
        transform: `translateX(${x * (1 - merge)}px) translateY(${interpolate(enter, [0, 1], [60, 0])}px) scale(${1 - merge * 0.6})`,
        filter: `blur(${merge * 8}px)`,
      }}
    >
      <svg width="88" height="88" viewBox="0 0 24 24">
        <path d={icon.path} fill="#fff" />
      </svg>
      <div>
        <div style={{ fontFamily: display, fontSize: 54, fontWeight: 600, letterSpacing: '-0.03em', color: color.text }}>{name}</div>
        <div style={{ fontFamily: display, fontSize: 28, color: color.muted, marginTop: 4 }}>{role}</div>
      </div>
    </div>
  )
}

/** Linear + Notion collapse into one Orbit. */
export function Merge() {
  const frame = useCurrentFrame()
  const plus = interpolate(frame, [18, 28, 58, 66], [0, 1, 1, 0], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
  const orbit = interpolate(frame, [70, 72], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
  const flash = interpolate(frame, [72, 80, 110], [0, 1, 0.35], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', paddingTop: 150 }}>
        <Reveal text="Two tools. *One* workspace." size={92} weight={700} align="center" stagger={4} />
      </AbsoluteFill>
      <ToolCard icon={siLinear} name="Linear" role="Issues & roadmaps" x={-330} from={6} />
      <ToolCard icon={siNotion} name="Notion" role="Docs & wikis" x={330} from={12} />
      <div style={{ position: 'absolute', left: '50%', top: 490, marginLeft: -30, width: 60, textAlign: 'center', fontFamily: display, fontSize: 72, fontWeight: 300, color: color.muted, opacity: plus }}>+</div>
      <AbsoluteFill style={{ alignItems: 'center', top: 380, opacity: orbit }}>
        <div style={{ position: 'absolute', width: 520, height: 520, marginTop: -120, borderRadius: 999, background: 'radial-gradient(circle, rgba(242,69,143,0.55), transparent 65%)', opacity: flash }} />
        <Sequence from={70} layout="none">
          <div style={{ display: 'flex', alignItems: 'center', gap: 24 }}>
              <OrbitMark size={140} />
              <Wordmark size={130} from={6} />
            </div>
        </Sequence>
      </AbsoluteFill>
      <AbsoluteFill style={{ alignItems: 'center', top: 660 }}>
        <FadeUp from={86}>
          <div style={{ fontFamily: mono, fontSize: 26, color: color.muted, letterSpacing: '0.02em' }}>self-hosted · open source · one binary</div>
        </FadeUp>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

/** Chapter divider: big index number, name and the product it replaces. */
export function Chapter({ index, name, line }: { index: string; name: string; line: string }) {
  const frame = useCurrentFrame()
  const { durationInFrames } = useVideoConfig()
  const bar = interpolate(frame, [4, 30], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: ease })
  return (
    <AbsoluteFill>
      <Background intensity={0.7} />
      <AbsoluteFill style={{ justifyContent: 'center', paddingLeft: 200, gap: 26, transform: `translateX(${interpolate(frame, [0, durationInFrames], [0, -30])}px)` }}>
        <FadeUp from={0}>
          <div style={{ fontFamily: mono, fontSize: 30, color: color.pink, letterSpacing: '0.2em' }}>{index}</div>
        </FadeUp>
        <div style={{ height: 3, width: 180 * bar, background: `linear-gradient(90deg, ${color.pink}, ${color.violet})`, borderRadius: 9 }} />
        <Reveal text={name} size={150} weight={700} from={6} />
        <Reveal text={line} size={46} weight={500} from={16} stagger={2} style={{ color: color.muted }} />
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

export const TasksChapter = () => <Chapter index="01 — TASKS" name="Plan it." line="Everything you love about *Linear,* on your server." />
export const DocsChapter = () => <Chapter index="02 — DOCS" name="Write it down." line="Everything you love about *Notion,* next to your tasks." />
