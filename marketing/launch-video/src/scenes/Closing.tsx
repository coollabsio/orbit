import type { ReactNode } from 'react'
import { CalendarDays, GitMerge, KeyRound, Lock, RotateCcw, Scale } from 'lucide-react'
import { siDiscord, siGithub, siRust, siSqlite } from 'simple-icons'
import { AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion'
import { Background } from '../components/Background'
import { OrbitMark, Wordmark } from '../components/Logo'
import { Eyebrow, FadeUp, gradientText, Reveal } from '../components/Text'
import { color, display, mono } from '../theme'

function Brand({ icon, size = 44 }: { icon: { path: string; hex: string }; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24">
      <path d={icon.path} fill="#fff" />
    </svg>
  )
}

type Tile = { title: string; body: string; media: ReactNode }

const TILES: Tile[] = [
  {
    title: 'GitHub sync',
    body: 'Label an issue or PR and it becomes a task. Merged PRs close it.',
    media: <Brand icon={siGithub} />,
  },
  {
    title: 'Discord to tasks',
    body: 'Turn Discord messages into tasks in the right project.',
    media: <Brand icon={siDiscord} />,
  },
  {
    title: 'Scoped API tokens',
    body: 'Per-project read/write tokens with expiry and service accounts.',
    media: <KeyRound size={44} color="#fff" strokeWidth={1.6} />,
  },
  {
    title: 'My week',
    body: 'Your assigned work for the week, overdue and due soon at a glance.',
    media: <CalendarDays size={44} color="#fff" strokeWidth={1.6} />,
  },
  {
    title: 'Relations & duplicates',
    body: 'Mark blockers, link related work and fold duplicates together.',
    media: <GitMerge size={44} color="#fff" strokeWidth={1.6} />,
  },
  {
    title: 'Trash & restore',
    body: 'Deleted tasks and projects wait 30 days before they are gone.',
    media: <RotateCcw size={44} color="#fff" strokeWidth={1.6} />,
  },
]

export function Grid() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', paddingTop: 96, gap: 18 }}>
        <Eyebrow>And there is more</Eyebrow>
        <Reveal text="Everything a shipping team *needs.*" size={76} align="center" from={4} />
      </AbsoluteFill>
      <div style={{ position: 'absolute', top: 360, left: 150, right: 150, display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 28 }}>
        {TILES.map((tile, index) => {
          const p = spring({ frame: frame - 16 - index * 5, fps, config: { damping: 18, stiffness: 110 } })
          const glow = interpolate(frame, [40 + index * 12, 52 + index * 12, 70 + index * 12], [0, 1, 0.25], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
          return (
            <div
              key={tile.title}
              style={{
                height: 300,
                padding: 34,
                borderRadius: 22,
                display: 'flex',
                flexDirection: 'column',
                gap: 18,
                background: 'linear-gradient(180deg, rgba(255,255,255,0.06), rgba(255,255,255,0.02))',
                border: `1px solid rgba(242,69,143,${0.12 + glow * 0.45})`,
                boxShadow: `0 0 ${60 * glow}px -10px rgba(242,69,143,0.6)`,
                opacity: p,
                transform: `translateY(${interpolate(p, [0, 1], [50, 0])}px) scale(${interpolate(p, [0, 1], [0.94, 1])})`,
              }}
            >
              <div style={{ width: 76, height: 76, borderRadius: 18, display: 'grid', placeItems: 'center', background: `linear-gradient(135deg, ${color.pinkDeep}, ${color.violet})`, boxShadow: '0 10px 30px rgba(242,69,143,0.35)' }}>
                {tile.media}
              </div>
              <div style={{ fontFamily: display, fontSize: 36, fontWeight: 600, letterSpacing: '-0.03em', color: color.text }}>{tile.title}</div>
              <div style={{ fontFamily: display, fontSize: 24, lineHeight: 1.4, color: color.muted }}>{tile.body}</div>
            </div>
          )
        })}
      </div>
    </AbsoluteFill>
  )
}

const TERMINAL = [
  { at: 8, text: '$ orbit serve', prompt: true },
  { at: 40, text: '✓ database migrated (SQLite)' },
  { at: 48, text: '✓ web app embedded' },
  { at: 56, text: '→ listening on http://0.0.0.0:8080' },
]

const FACTS: { icon: ReactNode; title: string; body: string }[] = [
  { icon: <Brand icon={siRust} size={34} />, title: 'One Rust binary', body: 'API and web app in a single process.' },
  { icon: <Brand icon={siSqlite} size={34} />, title: 'SQLite inside', body: 'No database server to run. Backups built in.' },
  { icon: <Lock size={32} strokeWidth={1.8} />, title: 'Your data stays home', body: 'Runs on your own server. No vendor lock-in.' },
  { icon: <Scale size={32} strokeWidth={1.8} />, title: 'Open source', body: 'Apache 2.0 licensed, on GitHub.' },
]

export function SelfHost() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const win = spring({ frame, fps, config: { damping: 20, stiffness: 90 } })
  return (
    <AbsoluteFill>
      <Background />
      <div style={{ position: 'absolute', left: 120, top: 150 }}>
        <Eyebrow>Self-hosted</Eyebrow>
        <div style={{ height: 24 }} />
        <Reveal text="Runs anywhere. *Owned* by you." size={80} from={4} style={{ maxWidth: 900 }} />
      </div>
      <div
        style={{
          position: 'absolute',
          left: 120,
          top: 440,
          width: 820,
          borderRadius: 18,
          overflow: 'hidden',
          background: '#0c0c0f',
          border: '1px solid rgba(255,255,255,0.12)',
          boxShadow: '0 40px 100px rgba(0,0,0,0.7), 0 0 120px -30px rgba(139,92,246,0.5)',
          opacity: win,
          transform: `translateY(${interpolate(win, [0, 1], [60, 0])}px)`,
        }}
      >
        <div style={{ height: 44, display: 'flex', alignItems: 'center', gap: 8, padding: '0 16px', background: '#131317', borderBottom: '1px solid rgba(255,255,255,0.07)' }}>
          {['#ff5f57', '#febc2e', '#28c840'].map((dot) => (
            <span key={dot} style={{ width: 12, height: 12, borderRadius: 99, background: dot }} />
          ))}
          <span style={{ marginLeft: 16, fontFamily: mono, fontSize: 15, color: color.muted }}>orbit — ssh</span>
        </div>
        <div style={{ padding: '30px 34px', minHeight: 300, fontFamily: mono, fontSize: 28, lineHeight: 1.8 }}>
          {TERMINAL.map((line) => {
            const chars = line.prompt ? Math.max(0, Math.floor((frame - line.at) / 1.6)) : frame >= line.at ? line.text.length : 0
            if (chars === 0 && !line.prompt) return null
            const text = line.text.slice(0, chars)
            return (
              <div key={line.text} style={{ color: line.prompt ? color.text : line.text.startsWith('→') ? '#ff9cc8' : color.green }}>
                {text}
                {line.prompt && chars < line.text.length ? <span style={{ background: color.text, marginLeft: 2 }}>&nbsp;</span> : null}
              </div>
            )
          })}
        </div>
      </div>
      <div style={{ position: 'absolute', right: 120, top: 400, width: 700, display: 'flex', flexDirection: 'column', gap: 22 }}>
        {FACTS.map((fact, index) => (
          <FadeUp key={fact.title} from={30 + index * 10} distance={30}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 24, padding: '22px 26px', borderRadius: 18, background: 'rgba(255,255,255,0.04)', border: `1px solid ${color.faint}` }}>
              <div style={{ width: 64, height: 64, flex: 'none', borderRadius: 16, display: 'grid', placeItems: 'center', color: '#fff', background: 'rgba(242,69,143,0.14)', border: '1px solid rgba(242,69,143,0.35)' }}>{fact.icon}</div>
              <div>
                <div style={{ fontFamily: display, fontSize: 32, fontWeight: 600, letterSpacing: '-0.03em', color: color.text }}>{fact.title}</div>
                <div style={{ fontFamily: display, fontSize: 23, color: color.muted, marginTop: 4 }}>{fact.body}</div>
              </div>
            </div>
          </FadeUp>
        ))}
      </div>
    </AbsoluteFill>
  )
}

export function Outro() {
  const frame = useCurrentFrame()
  const { durationInFrames } = useVideoConfig()
  const fadeOut = interpolate(frame, [durationInFrames - 20, durationInFrames], [1, 0], { extrapolateLeft: 'clamp' })
  return (
    <AbsoluteFill style={{ opacity: fadeOut }}>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', justifyContent: 'center', gap: 34, transform: `scale(${interpolate(frame, [0, 150], [0.97, 1.03])})` }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 26 }}>
          <OrbitMark size={130} />
          <Wordmark size={130} from={10} />
        </div>
        <FadeUp from={26}>
          <div style={{ fontFamily: display, fontSize: 60, fontWeight: 600, letterSpacing: '-0.04em', color: color.text }}>
            The only workspace <span style={gradientText}>you need.</span>
          </div>
        </FadeUp>
        <FadeUp from={36}>
          <div style={{ marginTop: -14, fontFamily: mono, fontSize: 26, letterSpacing: '0.02em', color: color.muted }}>Tasks · Docs · Mail & Chat coming soon · On your server</div>
        </FadeUp>
        <FadeUp from={48}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 18 }}>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 14,
                padding: '16px 30px',
                borderRadius: 14,
                background: `linear-gradient(135deg, ${color.pink}, ${color.pinkDeep})`,
                boxShadow: '0 16px 40px -8px rgba(242,69,143,0.6)',
                fontFamily: display,
                fontSize: 28,
                fontWeight: 600,
                color: '#fff',
              }}
            >
              <Brand icon={siGithub} size={30} />
              github.com/coollabsio/orbit
            </div>
            <div style={{ padding: '16px 24px', borderRadius: 14, border: `1px solid ${color.faint}`, fontFamily: display, fontSize: 26, color: color.muted }}>Now in public alpha</div>
          </div>
        </FadeUp>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

