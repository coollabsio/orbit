import type { CSSProperties, ReactElement } from 'react'
import { AbsoluteFill, Audio, interpolate, Sequence, spring, staticFile, useCurrentFrame, useVideoConfig } from 'remotion'
import { AppWindow, type CameraKey } from './components/AppWindow'
import { Background } from './components/Background'
import { OrbitMark, Wordmark } from './components/Logo'
import { FadeUp, gradientText, Reveal } from './components/Text'
import { color, display, ease, easeOut, mono, shot } from './theme'

/**
 * 44-second promo cut to scripts/make-beat.mjs (120 BPM: one beat = 15 frames, one bar = 60 frames).
 * Every scene starts on a beat; pink wipes hide the cuts.
 */

const BEAT = 15
const DROP = 120 // the full beat starts
const clamp = { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' } as const

type Feature = { label: string; shot?: string; path?: string; camera?: CameraKey[]; frames?: number }

const SLIDE = 90
const FEATURES: Feature[] = [
  { label: 'Tasks', shot: 'board', path: 'tasks', camera: [{ f: 0, z: 1, x: 800, y: 500 }, { f: SLIDE, z: 1.06, x: 560, y: 380 }] },
  { label: 'Timeline', shot: 'roadmap', path: 'tasks', camera: [{ f: 0, z: 1, x: 800, y: 500 }, { f: SLIDE, z: 1.06, x: 1150, y: 330 }] },
  { label: 'Docs', shot: 'docs-rich', path: 'docs', camera: [{ f: 0, z: 1, x: 800, y: 500 }, { f: SLIDE, z: 1.06, x: 900, y: 640 }] },
  { label: 'Chat', shot: 'chat', path: 'chat', camera: [{ f: 0, z: 1, x: 800, y: 500 }, { f: SLIDE, z: 1.06, x: 1000, y: 560 }] },
  { label: 'Mail', shot: 'mail', path: 'mail', camera: [{ f: 0, z: 1, x: 800, y: 500 }, { f: SLIDE, z: 1.06, x: 1150, y: 560 }] },
  { label: 'AI', frames: 150 }, // longer, so the question and the answer can be read
]

type Scene = { id: string; frames: number; render: (index: number) => ReactElement }

const SCENES: Scene[] = [
  { id: 'intro', frames: 120, render: () => <Intro /> },
  { id: 'apps', frames: 120, render: () => <Apps /> },
  { id: 'replaces', frames: 120, render: () => <Replaces /> },
  ...FEATURES.map((feature, index) => ({ id: feature.label, frames: feature.frames ?? SLIDE, render: () => <FeatureSlide feature={feature} index={index} /> })),
  { id: 'selfhost', frames: 120, render: () => <SelfHost /> },
  { id: 'cloud', frames: 90, render: () => <Cloud /> },
  { id: 'outro', frames: 150, render: () => <Outro /> },
]

const STARTS = SCENES.map((_, index) => SCENES.slice(0, index).reduce((sum, scene) => sum + scene.frames, 0))
export const PROMO_FRAMES = SCENES.reduce((sum, scene) => sum + scene.frames, 0)
const END = STARTS[SCENES.length - 1] // the beat stops on the end card

export function Promo() {
  return (
    <AbsoluteFill style={{ backgroundColor: color.bg }}>
      <Audio src={staticFile('audio/promo-beat.wav')} />
      {SCENES.map((scene, index) => (
        <Sequence key={scene.id} from={STARTS[index]} durationInFrames={scene.frames}>
          {scene.render(index)}
        </Sequence>
      ))}
      <Pulse />
      {STARTS.slice(1).map((cut, index) => (
        <Wipe key={cut} at={cut} flip={index % 2 === 1} />
      ))}
    </AbsoluteFill>
  )
}

/** A soft brand glow that flashes on every kick. */
function Pulse() {
  const frame = useCurrentFrame()
  if (frame < DROP || frame >= END) return null
  const hit = Math.exp(-((frame - DROP) % BEAT) / 3)
  return (
    <AbsoluteFill
      style={{
        pointerEvents: 'none',
        opacity: hit * 0.35,
        background: 'radial-gradient(ellipse 60% 50% at 50% 110%, rgba(242,69,143,0.55), transparent 70%)',
      }}
    />
  )
}

/** Two stacked brand panels sweep across the frame and hide the hard cut at `at`. */
function Wipe({ at, flip }: { at: number; flip: boolean }) {
  const frame = useCurrentFrame()
  const span = 6
  if (frame < at - span || frame > at + span + 3) return null
  const panel = (delay: number, fill: string) => {
    const t = interpolate(frame, [at - span + delay, at + span + delay], [0, 1], clamp)
    const head = ease(Math.min(1, t * 2)) * 100
    const tail = ease(Math.max(0, t * 2 - 1)) * 100
    const left = flip ? 100 - head : tail
    const right = flip ? tail : 100 - head
    return <AbsoluteFill style={{ background: fill, clipPath: `inset(0 ${right}% 0 ${left}%)` }} />
  }
  return (
    <AbsoluteFill style={{ pointerEvents: 'none' }}>
      {panel(0, color.pinkShadow)}
      {panel(2, color.pink)}
    </AbsoluteFill>
  )
}

/** A word that pops in on a beat: scale overshoot plus unblur. */
function Pop({ at, children, size, style }: { at: number; children: string; size: number; style?: CSSProperties }) {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const p = spring({ frame: frame - at, fps, config: { damping: 13, stiffness: 190 } })
  return (
    <span
      style={{
        display: 'inline-block',
        fontFamily: display,
        fontSize: size,
        fontWeight: 700,
        letterSpacing: '-0.045em',
        color: color.text,
        opacity: Math.min(1, p * 1.5),
        transform: `translateY(${interpolate(p, [0, 1], [40, 0])}px) scale(${interpolate(p, [0, 1], [0.7, 1])})`,
        filter: `blur(${interpolate(Math.min(p, 1), [0, 1], [12, 0])}px)`,
        ...style,
      }}
    >
      {children}
    </span>
  )
}

function Intro() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const shift = spring({ frame: frame - 30, fps, config: { damping: 200 }, durationInFrames: 24 })
  const push = interpolate(frame, [0, 120], [1.1, 1], { easing: easeOut, ...clamp })
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', justifyContent: 'center', transform: `scale(${push})` }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: interpolate(shift, [0, 1], [0, 44]) }}>
          <OrbitMark size={190} from={8} />
          <div style={{ width: interpolate(shift, [0, 1], [0, 520]), overflow: 'hidden' }}>
            <Wordmark size={190} from={34} />
          </div>
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

const APPS = ['Tasks', 'Docs', 'Chat', 'Mail', 'AI']

/** The five apps land one per beat, then "One app." on the next bar. */
function Apps() {
  const frame = useCurrentFrame()
  const settle = interpolate(frame, [60, 72], [0, 1], { easing: ease, ...clamp })
  return (
    <AbsoluteFill>
      <Background intensity={0.7} />
      <AbsoluteFill style={{ alignItems: 'center', justifyContent: 'center', gap: 30 }}>
        <div style={{ display: 'flex', gap: 54, transform: `translateY(${-settle * 20}px) scale(${1 - settle * 0.35})` }}>
          {APPS.map((label, index) => (
            <Pop key={label} at={index * 11} size={150} style={{ color: settle > 0 ? `rgba(246,246,247,${1 - settle * 0.45})` : color.text }}>
              {label}
            </Pop>
          ))}
        </div>
        {frame >= 60 ? (
          <Pop at={60} size={170} style={gradientText}>
            One app.
          </Pop>
        ) : null}
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

const REPLACED = ['Linear', 'Notion', 'Slack', 'Discord']

/** Four tools appear on the first bar and are struck out on the second. */
function Replaces() {
  const frame = useCurrentFrame()
  return (
    <AbsoluteFill>
      <Background intensity={0.6} />
      <AbsoluteFill style={{ alignItems: 'center', justifyContent: 'center', gap: 56 }}>
        <FadeUp>
          <div style={{ fontFamily: mono, fontSize: 30, letterSpacing: '0.2em', textTransform: 'uppercase', color: color.pink }}>Replaces</div>
        </FadeUp>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, auto)', gap: 80 }}>
          {REPLACED.map((name, index) => {
            const strike = interpolate(frame, [60 + index * 7, 66 + index * 7], [0, 1], { easing: ease, ...clamp })
            return (
              <div key={name} style={{ position: 'relative' }}>
                <Pop at={index * BEAT} size={120} style={{ color: strike > 0 ? `rgba(154,154,163,${1 - strike * 0.4})` : color.text }}>
                  {name}
                </Pop>
                <div
                  style={{
                    position: 'absolute',
                    left: -10,
                    right: -10,
                    top: '54%',
                    height: 10,
                    borderRadius: 6,
                    background: color.pink,
                    boxShadow: `0 0 24px ${color.pink}`,
                    transformOrigin: 'left center',
                    transform: `scaleX(${strike}) rotate(-4deg)`,
                  }}
                />
              </div>
            )
          })}
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

function FeatureSlide({ feature, index }: { feature: Feature; index: number }) {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const enter = spring({ frame: frame - 2, fps, config: { damping: 22, stiffness: 120 } })
  const tilt = interpolate(frame, [0, feature.frames ?? SLIDE], [-9, -4])
  return (
    <AbsoluteFill>
      <Background intensity={0.8} />
      <div style={{ position: 'absolute', left: 130, top: 0, bottom: 0, width: 520, display: 'flex', flexDirection: 'column', justifyContent: 'center', gap: 24 }}>
        <FadeUp from={2} distance={16}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 18, fontFamily: mono, fontSize: 26, color: color.pink, letterSpacing: '0.08em' }}>
            <span>{String(index + 1).padStart(2, '0')}</span>
            <span style={{ width: interpolate(enter, [0, 1], [0, 64]), height: 2, background: color.pink }} />
          </div>
        </FadeUp>
        <Reveal text={feature.label} size={124} weight={700} from={4} />
      </div>
      <div
        style={{
          position: 'absolute',
          left: 670,
          top: 540,
          perspective: 2200,
          transform: `translate(${interpolate(enter, [0, 1], [420, 0])}px, -50%)`,
          opacity: Math.min(1, enter * 1.6),
        }}
      >
        <div style={{ transform: `rotateY(${tilt}deg) rotateX(4deg)`, transformOrigin: 'left center' }}>
          {feature.shot ? <AppWindow src={shot(feature.shot)} width={1170} url={`app.orbit.so/${feature.path}`} camera={feature.camera} /> : <AiPanel />}
        </div>
      </div>
      <Progress active={index} />
    </AbsoluteFill>
  )
}

function Progress({ active }: { active: number }) {
  const frame = useCurrentFrame()
  const fill = interpolate(frame, [0, FEATURES[active].frames ?? SLIDE], [0, 1], clamp)
  return (
    <div style={{ position: 'absolute', left: 130, bottom: 86, display: 'flex', gap: 12 }}>
      {FEATURES.map((feature, index) => (
        <div key={feature.label} style={{ width: 72, height: 3, borderRadius: 2, background: 'rgba(255,255,255,0.12)', overflow: 'hidden' }}>
          <div style={{ height: '100%', width: `${(index < active ? 1 : index === active ? fill : 0) * 100}%`, background: color.pink }} />
        </div>
      ))}
    </div>
  )
}

const QUESTION = 'What is blocking the launch?'
const SOURCES = [
  { kind: 'Task', name: 'ORB-5 Realtime updates', tint: color.pink },
  { kind: 'Doc', name: 'Architecture overview', tint: color.violet },
  { kind: 'Chat', name: '#engineering', tint: color.green },
  { kind: 'Mail', name: 'Moving our team to Orbit', tint: '#f5a524' },
]

/** Concept UI for the AI assistant: a typed question, then an answer built from sources across the app. */
function AiPanel() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const typed = QUESTION.slice(0, Math.round(interpolate(frame, [10, 50], [0, QUESTION.length], clamp)))
  const answer = spring({ frame: frame - 60, fps, config: { damping: 200 }, durationInFrames: 20 })
  return (
    <div
      style={{
        width: 1140,
        padding: 48,
        borderRadius: 28,
        display: 'flex',
        flexDirection: 'column',
        gap: 32,
        background: 'linear-gradient(180deg, rgba(28,24,32,0.96), rgba(14,12,18,0.96))',
        border: '1px solid rgba(255,255,255,0.12)',
        boxShadow: '0 50px 120px -20px rgba(0,0,0,0.85), 0 0 160px -30px rgba(242,69,143,0.55)',
        fontFamily: display,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 18, padding: '24px 28px', borderRadius: 18, background: 'rgba(255,255,255,0.05)', border: '1px solid rgba(242,69,143,0.4)' }}>
        <svg width={36} height={36} viewBox="0 0 24 24">
          <path d="M12 2l2.2 6.6L21 11l-6.8 2.4L12 20l-2.2-6.6L3 11l6.8-2.4z" fill={color.pink} />
        </svg>
        <span style={{ fontSize: 36, color: color.text, letterSpacing: '-0.02em' }}>
          {typed}
          <span style={{ opacity: frame % 16 < 8 ? 1 : 0, color: color.pink }}>|</span>
        </span>
      </div>
      <div style={{ opacity: answer, transform: `translateY(${interpolate(answer, [0, 1], [16, 0])}px)`, fontSize: 32, lineHeight: 1.4, color: color.muted }}>
        <span style={{ color: color.text }}>One thing:</span> realtime updates are still in progress. Everything else is done.
      </div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 14 }}>
        {SOURCES.map((source, index) => {
          const p = spring({ frame: frame - 85 - index * 7, fps, config: { damping: 14, stiffness: 180 } })
          return (
            <div
              key={source.kind}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 12,
                padding: '12px 18px',
                borderRadius: 999,
                background: 'rgba(255,255,255,0.05)',
                border: `1px solid ${source.tint}66`,
                opacity: Math.min(1, p),
                transform: `scale(${interpolate(p, [0, 1], [0.6, 1])})`,
                fontSize: 23,
              }}
            >
              <span style={{ fontFamily: mono, fontSize: 16, color: source.tint, textTransform: 'uppercase', letterSpacing: '0.08em' }}>{source.kind}</span>
              <span style={{ color: color.text }}>{source.name}</span>
            </div>
          )
        })}
      </div>
    </div>
  )
}

const FACTS = [
  { big: 'Tiny', small: 'a few dozen MB of RAM' },
  { big: '1 binary', small: 'Rust server + web app' },
  { big: 'SQLite', small: 'or Postgres' },
  { big: 'Your data', small: 'on your server' },
]

function SelfHost() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', justifyContent: 'center', gap: 70 }}>
        <Reveal text="*Self-hosted.*" size={140} weight={700} align="center" from={0} />
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 400px)', gap: 28 }}>
          {FACTS.map((fact, index) => {
            const p = spring({ frame: frame - BEAT * (index + 2), fps, config: { damping: 15, stiffness: 170 } })
            return (
              <div
                key={fact.big}
                style={{
                  padding: '40px 36px',
                  borderRadius: 24,
                  background: 'linear-gradient(180deg, rgba(255,255,255,0.07), rgba(255,255,255,0.02))',
                  border: '1px solid rgba(242,69,143,0.3)',
                  opacity: Math.min(1, p * 1.4),
                  transform: `translateY(${interpolate(p, [0, 1], [50, 0])}px) scale(${interpolate(p, [0, 1], [0.85, 1])})`,
                  fontFamily: display,
                }}
              >
                <div style={{ fontSize: 60, fontWeight: 700, letterSpacing: '-0.04em', color: color.text }}>{fact.big}</div>
                <div style={{ fontSize: 28, color: color.muted, marginTop: 8 }}>{fact.small}</div>
              </div>
            )
          })}
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

function Cloud() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const price = spring({ frame: frame - BEAT, fps, config: { damping: 12, stiffness: 160 } })
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', justifyContent: 'center', gap: 34 }}>
        <FadeUp>
          <div style={{ fontFamily: mono, fontSize: 30, letterSpacing: '0.2em', textTransform: 'uppercase', color: color.pink }}>Or use Orbit Cloud</div>
        </FadeUp>
        <div
          style={{
            display: 'flex',
            alignItems: 'baseline',
            gap: 24,
            fontFamily: display,
            opacity: Math.min(1, price * 1.4),
            transform: `scale(${interpolate(price, [0, 1], [0.7, 1])})`,
          }}
        >
          <span style={{ fontSize: 260, fontWeight: 700, letterSpacing: '-0.04em', paddingRight: '0.08em', ...gradientText }}>$10</span>
          <span style={{ fontSize: 56, fontWeight: 500, color: color.muted }}>/ workspace / month</span>
        </div>
        <FadeUp from={BEAT * 3}>
          <div style={{ fontFamily: display, fontSize: 38, color: color.text }}>Unlimited members.</div>
        </FadeUp>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

function Outro() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const link = spring({ frame: frame - 24, fps, config: { damping: 16, stiffness: 140 } })
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', justifyContent: 'center', gap: 50 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 44 }}>
          <OrbitMark size={170} from={2} />
          <Wordmark size={180} from={10} />
        </div>
        <div
          style={{
            padding: '18px 44px',
            borderRadius: 999,
            fontFamily: mono,
            fontSize: 46,
            fontWeight: 500,
            color: '#fff',
            background: `linear-gradient(100deg, ${color.pink}, ${color.pinkDeep})`,
            boxShadow: `0 0 70px -6px ${color.pink}`,
            opacity: Math.min(1, link),
            transform: `scale(${interpolate(link, [0, 1], [0.7, 1])})`,
          }}
        >
          orbit.so
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}
