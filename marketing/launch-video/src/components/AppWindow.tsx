import { createContext, useContext, type CSSProperties, type ReactNode } from 'react'
import { Img, interpolate, useCurrentFrame } from 'remotion'
import { color, ease, mono, shot, SHOT_H, SHOT_W, type Box } from '../theme'

/** A camera keyframe: at frame `f`, zoom `z` and centre the shot on (x, y) in shot CSS px. */
export type CameraKey = { f: number; z: number; x: number; y: number }

const ScaleContext = createContext(1)
/** Screen pixels per shot pixel inside the current window — lets overlays keep a constant on-screen size. */
export const useShotScale = () => useContext(ScaleContext)

export function sampleCamera(keys: CameraKey[], frame: number): CameraKey {
  if (frame <= keys[0].f) return keys[0]
  for (let i = 1; i < keys.length; i++) {
    const a = keys[i - 1]
    const b = keys[i]
    if (frame <= b.f) {
      const t = ease((frame - a.f) / (b.f - a.f))
      return { f: frame, z: a.z + (b.z - a.z) * t, x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }
    }
  }
  return keys[keys.length - 1]
}

const BAR = 44

/** A browser-style window showing a screenshot through an animated camera. Children are placed in shot coordinates. */
export function AppWindow({
  src,
  width,
  camera = [{ f: 0, z: 1, x: SHOT_W / 2, y: SHOT_H / 2 }],
  url = 'orbit.yourcompany.com/tasks',
  children,
  style,
  imageStyle,
}: {
  src: string
  width: number
  camera?: CameraKey[]
  url?: string
  children?: ReactNode
  style?: CSSProperties
  imageStyle?: CSSProperties
}) {
  const frame = useCurrentFrame()
  const height = (width * SHOT_H) / SHOT_W
  const cam = sampleCamera(camera, frame)
  const scale = (width / SHOT_W) * cam.z
  const tx = Math.min(0, Math.max(width - SHOT_W * scale, width / 2 - cam.x * scale))
  const ty = Math.min(0, Math.max(height - SHOT_H * scale, height / 2 - cam.y * scale))
  return (
    <div
      style={{
        width,
        borderRadius: 18,
        overflow: 'hidden',
        background: '#0d0d10',
        border: '1px solid rgba(255,255,255,0.12)',
        boxShadow: '0 50px 120px -20px rgba(0,0,0,0.85), 0 0 0 1px rgba(0,0,0,0.6), 0 0 140px -30px rgba(242,69,143,0.45)',
        ...style,
      }}
    >
      <div style={{ height: BAR, display: 'flex', alignItems: 'center', gap: 8, padding: '0 16px', background: '#111114', borderBottom: '1px solid rgba(255,255,255,0.07)' }}>
        {['#ff5f57', '#febc2e', '#28c840'].map((dot) => (
          <span key={dot} style={{ width: 12, height: 12, borderRadius: 99, background: dot, opacity: 0.9 }} />
        ))}
        <div
          style={{
            margin: '0 auto',
            padding: '5px 18px',
            borderRadius: 8,
            background: 'rgba(255,255,255,0.05)',
            color: color.muted,
            fontFamily: mono,
            fontSize: 14,
            transform: 'translateX(-30px)',
          }}
        >
          {url}
        </div>
      </div>
      <div style={{ position: 'relative', width, height, overflow: 'hidden', background: color.appCanvas }}>
        <div style={{ position: 'absolute', left: 0, top: 0, width: SHOT_W, height: SHOT_H, transformOrigin: '0 0', transform: `translate(${tx}px, ${ty}px) scale(${scale})` }}>
          <Img src={src} style={{ width: SHOT_W, height: SHOT_H, display: 'block', ...imageStyle }} />
          <ScaleContext.Provider value={scale}>{children}</ScaleContext.Provider>
        </div>
      </div>
    </div>
  )
}

/** Crops a region of a screenshot so it can be lifted and animated separately (e.g. a dragged card). */
export function Crop({ src, box, style }: { src: string; box: Box; style?: CSSProperties }) {
  return (
    <div style={{ position: 'absolute', left: box.x, top: box.y, width: box.w, height: box.h, overflow: 'hidden', ...style }}>
      <Img src={src} style={{ position: 'absolute', left: -box.x, top: -box.y, width: SHOT_W, height: SHOT_H, maxWidth: 'none' }} />
    </div>
  )
}

/** Spotlight: dims everything outside `box` and draws a glowing brand outline around it. */
export function Highlight({ box, from, to = Infinity, pad = 6, radius = 10, dim = 0.55 }: { box: Box; from: number; to?: number; pad?: number; radius?: number; dim?: number }) {
  const frame = useCurrentFrame()
  const scale = useShotScale()
  const clamp = { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' } as const
  const p = interpolate(frame, [from, from + 14], [0, 1], clamp) * (Number.isFinite(to) ? interpolate(frame, [to - 10, to], [1, 0], clamp) : 1)
  if (p <= 0) return null
  const line = 2 / scale
  return (
    <div
      style={{
        position: 'absolute',
        left: box.x - pad,
        top: box.y - pad,
        width: box.w + pad * 2,
        height: box.h + pad * 2,
        borderRadius: radius,
        opacity: p,
        border: `${line}px solid ${color.pink}`,
        boxShadow: `0 0 0 9999px rgba(0,0,0,${dim}), 0 0 ${30 / scale}px ${4 / scale}px rgba(242,69,143,0.5)`,
      }}
    />
  )
}

/** Crossfades a later screenshot (same 1600x1000 frame) over the window's base shot from frame `from`. */
export function ShotLayer({ name, from }: { name: string; from: number }) {
  const frame = useCurrentFrame()
  const opacity = interpolate(frame, [from, from + 8], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
  if (opacity <= 0) return null
  return <Img src={shot(name)} style={{ position: 'absolute', inset: 0, width: SHOT_W, height: SHOT_H, opacity }} />
}
