import { AbsoluteFill, Img, interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion'
import { AppWindow, Crop, Highlight, useShotScale } from '../components/AppWindow'
import { Background } from '../components/Background'
import { Cursor } from '../components/Cursor'
import { FeatureLayout } from '../components/FeatureLayout'
import { Body, Eyebrow, Reveal } from '../components/Text'
import { appFont, center, color, display, ease, region, shot, SHOT_H, SHOT_W } from '../theme'

const W = 1240

/** A small confirmation badge drawn at a constant on-screen size inside a window. */
function Badge({ x, y, from, text }: { x: number; y: number; from: number; text: string }) {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const scale = useShotScale()
  const p = spring({ frame: frame - from, fps, config: { damping: 14 } })
  if (frame < from) return null
  return (
    <div
      style={{
        position: 'absolute',
        left: x,
        top: y,
        transformOrigin: '0 0',
        transform: `scale(${(1 / scale) * p})`,
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        padding: '10px 16px',
        borderRadius: 12,
        background: '#15151a',
        border: '1px solid rgba(76,183,130,0.5)',
        boxShadow: '0 12px 30px rgba(0,0,0,0.6)',
        color: color.text,
        fontFamily: display,
        fontSize: 20,
        fontWeight: 500,
        whiteSpace: 'nowrap',
      }}
    >
      <span style={{ display: 'grid', placeItems: 'center', width: 22, height: 22, borderRadius: 99, background: color.green, color: '#062', fontSize: 14, fontWeight: 700 }}>✓</span>
      {text}
    </div>
  )
}

export function Board() {
  const frame = useCurrentFrame()
  const card = region('board', 'card')
  // Drop target: the free space under the last "In Progress" card.
  const drop = { x: 908, y: 832 }
  const grab = { x: card.x + card.w / 2, y: card.y + card.h / 2 }
  const lift = interpolate(frame, [58, 66, 104, 112], [0, 1, 1, 0], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
  const move = ease(interpolate(frame, [62, 102], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }))
  const dx = (drop.x - card.x) * move
  const dy = (drop.y - card.y) * move
  const dragging = frame >= 58
  return (
    <FeatureLayout
      eyebrow="Boards"
      title="Kanban that keeps *up.*"
      body="Drag cards across statuses and reorder within a column. Every move is saved the moment you drop it."
      chips={[{ label: 'List & board', dot: color.pink }, { label: 'Drag & drop', dot: color.violet }, { label: 'Filters & sorting', dot: color.green }]}
    >
      <AppWindow
        src={shot('board')}
        width={W}
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 40, z: 1.2, x: 700, y: 500 },
          { f: 165, z: 1.3, x: 760, y: 600 },
        ]}
      >
        {dragging ? (
          <div style={{ position: 'absolute', left: card.x, top: card.y, width: card.w, height: card.h, borderRadius: 8, background: color.appColumn, border: `1.5px dashed rgba(242,69,143,${0.6 * (1 - move)})` }} />
        ) : null}
        <Crop
          src={shot('board')}
          box={card}
          style={{
            left: card.x + dx,
            top: card.y + dy,
            borderRadius: 8,
            transform: `scale(${1 + lift * 0.05}) rotate(${lift * -2.5}deg)`,
            boxShadow: lift > 0 ? `0 ${24 * lift}px ${50 * lift}px rgba(0,0,0,0.7), 0 0 0 ${1.5 * lift}px ${color.pink}` : undefined,
            zIndex: 5,
          }}
        />
        <Cursor
          from={24}
          path={[
            { f: 24, x: 1150, y: 700 },
            { f: 54, x: grab.x + 40, y: grab.y + 6 },
            { f: 62, x: grab.x + 40, y: grab.y + 6 },
            { f: 102, x: drop.x + card.w / 2 + 40, y: drop.y + card.h / 2 + 6 },
            { f: 130, x: drop.x + card.w / 2 + 120, y: drop.y + card.h / 2 + 60 },
          ]}
          clicks={[58]}
        />
        <Badge x={drop.x + 10} y={drop.y + card.h + 14} from={108} text="Moved to In Progress" />
      </AppWindow>
    </FeatureLayout>
  )
}

export function Roadmap() {
  const today = region('roadmap', 'todayMarker')
  // "Wildcard domains…" range bar (measured from roadmap.png).
  const bar = { x: 932, y: 203, w: 240, h: 22 }
  return (
    <FeatureLayout
      side="right"
      eyebrow="Roadmap"
      title="See the whole *quarter.*"
      body="A timeline of every project with date ranges, milestones and today front and centre. Drag to reschedule."
      chips={[{ label: 'Week · Month · Quarter', dot: color.violet }, { label: 'Date ranges', dot: color.pink }, { label: 'Grouped by project', dot: color.green }]}
    >
      <AppWindow
        src={shot('roadmap')}
        width={W}
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 45, z: 1.45, x: 1050, y: 330 },
          { f: 150, z: 1.55, x: 1080, y: 320 },
        ]}
      >
        <Highlight box={today} from={50} to={88} pad={6} radius={14} dim={0.35} />
        <Highlight box={bar} from={96} pad={5} radius={8} dim={0.35} />
        <Cursor from={60} path={[{ f: 60, x: 1250, y: 520 }, { f: 92, x: bar.x + bar.w - 30, y: bar.y + 14 }, { f: 150, x: bar.x + bar.w - 24, y: bar.y + 16 }]} clicks={[98]} />
      </AppWindow>
    </FeatureLayout>
  )
}

export function Detail() {
  const checklist = region('detail', 'checklist')
  const relations = region('detail', 'relations')
  const firstComment = { x: 264, y: 472, w: 760, h: 178 }
  return (
    <FeatureLayout
      eyebrow="Task detail"
      title="All the context, one *click* away."
      body="Descriptions with checklists, assignees, labels, due ranges, relations and threaded comments, right beside the work."
      chips={[{ label: 'Checklists', dot: color.green }, { label: 'Blocks & relates', dot: color.violet }, { label: 'Threaded comments', dot: color.pink }, { label: 'Attachments', dot: '#f2c94c' }]}
    >
      <AppWindow
        src={shot('detail')}
        width={W}
        url="orbit.yourcompany.com/tasks/ORB-6C28"
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 32, z: 1.6, x: 640, y: 250 },
          { f: 88, z: 1.6, x: 640, y: 300 },
          { f: 118, z: 1.55, x: 640, y: 560 },
          { f: 150, z: 1.6, x: 640, y: 570 },
        ]}
      >
        <Highlight box={checklist} from={36} to={66} pad={8} />
        <Highlight box={relations} from={66} to={96} pad={4} />
        <Highlight box={firstComment} from={112} pad={0} radius={12} />
      </AppWindow>
    </FeatureLayout>
  )
}

/** ⌘K: keycaps, then the palette opens and the query is typed. */
export function Palette() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const dialog = region('palette-docs', 'dialog')
  const results = region('palette-docs', 'results')
  const query = 'roadmap'
  const typed = query.slice(0, Math.max(0, Math.floor((frame - 44) / 3)))
  const reveal = interpolate(frame, [62, 84], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
  const open = interpolate(frame, [30, 40], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
  const pop = spring({ frame: frame - 30, fps, config: { damping: 16, stiffness: 160 } })
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', paddingTop: 80, gap: 18 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 26 }}>
          <Reveal text="One search for everything" size={76} align="center" />
          <Keys />
        </div>
        <Body from={10} width={1000}>
          <span style={{ display: 'block', textAlign: 'center' }}>Tasks, pages and actions in one palette. Never leave the keyboard.</span>
        </Body>
      </AbsoluteFill>
      <AbsoluteFill style={{ alignItems: 'center', top: 320 }}>
        <div style={{ opacity: interpolate(frame, [0, 14], [0, 1], { extrapolateRight: 'clamp' }), transform: `translateY(${interpolate(frame, [0, 20], [60, 0], { extrapolateRight: 'clamp' })}px)` }}>
          <AppWindow
            src={shot('list')}
            width={1180}
            camera={[
              { f: 0, z: 1, x: 800, y: 500 },
              { f: 30, z: 1.9, x: 800, y: 240 },
              { f: 120, z: 2, x: 800, y: 235 },
            ]}
          >
            <Img src={shot('palette-docs')} style={{ position: 'absolute', inset: 0, width: SHOT_W, height: SHOT_H, opacity: open }} />
            <div
              style={{
                position: 'absolute',
                left: dialog.x,
                top: dialog.y,
                width: dialog.w,
                height: dialog.h,
                overflow: 'hidden',
                opacity: open,
                transform: `scale(${interpolate(pop, [0, 1], [0.94, 1])})`,
                boxShadow: '0 30px 80px rgba(0,0,0,0.6)',
                borderRadius: 12,
              }}
            >
              <Img src={shot('palette-docs')} style={{ position: 'absolute', left: -dialog.x, top: -dialog.y, width: SHOT_W, height: SHOT_H, maxWidth: 'none' }} />
              {/* Re-type the query over the captured one. */}
              <div style={{ position: 'absolute', left: 541 - dialog.x, top: 130 - dialog.y, width: 300, height: 22, background: color.appInput, fontFamily: appFont, fontSize: 15, lineHeight: '22px', color: '#ececec', paddingLeft: 6 }}>
                {typed}
                <span style={{ display: 'inline-block', width: 1.5, height: 17, marginLeft: 1, verticalAlign: 'middle', background: color.pink, opacity: Math.floor(frame / 8) % 2 ? 1 : 0.2 }} />
              </div>
              <div style={{ position: 'absolute', left: results.x - dialog.x, top: results.y - dialog.y + results.h * reveal, width: results.w, height: results.h * (1 - reveal) + 4, background: color.appCanvas }} />
            </div>
          </AppWindow>
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}

function Keys() {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  return (
    <div style={{ display: 'flex', gap: 12 }}>
      {['⌘', 'K'].map((key, index) => {
        const p = spring({ frame: frame - 10 - index * 5, fps, config: { damping: 12 } })
        const press = frame >= 28 && frame < 34 ? 5 : 0
        return (
          <div
            key={key}
            style={{
              display: 'grid',
              placeItems: 'center',
              width: 92,
              height: 92,
              borderRadius: 18,
              background: 'linear-gradient(180deg, #2a2a31, #17171b)',
              border: '1px solid rgba(255,255,255,0.14)',
              boxShadow: `0 ${8 - press}px 0 #0b0b0d, 0 ${14 - press}px 30px rgba(0,0,0,0.5)`,
              transform: `translateY(${press}px) scale(${p})`,
              fontFamily: display,
              fontSize: 48,
              fontWeight: 500,
              color: color.text,
            }}
          >
            {key}
          </div>
        )
      })}
    </div>
  )
}

export function Bulk() {
  const rows = region('bulk', 'selectedRows')
  const bar = region('bulk', 'bulkBar')
  return (
    <FeatureLayout
      side="right"
      eyebrow="Bulk actions"
      title="Change a hundred tasks in *one* move."
      body="Select tasks and update status, priority, assignees or labels together, in a single atomic request."
      chips={[{ label: 'Status', dot: '#f2c94c' }, { label: 'Priority', dot: '#eb5757' }, { label: 'Assignee', dot: color.green }, { label: 'Labels', dot: color.violet }]}
    >
      <AppWindow
        src={shot('bulk')}
        width={W}
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 34, z: 1.2, x: 800, y: 300 },
          { f: 70, z: 1.2, x: 800, y: 300 },
          { f: 100, z: 1.6, x: center(bar).x, y: 820 },
          { f: 120, z: 1.65, x: center(bar).x, y: 830 },
        ]}
      >
        <Highlight box={rows} from={30} to={72} pad={2} radius={6} dim={0.4} />
        <Highlight box={bar} from={80} pad={6} radius={14} dim={0.45} />
        <Cursor from={70} path={[{ f: 70, x: 1100, y: 700 }, { f: 98, x: bar.x + 175, y: bar.y + 24 }, { f: 120, x: bar.x + 178, y: bar.y + 26 }]} clicks={[102]} />
      </AppWindow>
    </FeatureLayout>
  )
}

export function Workflow() {
  const card = region('workflow', 'statusesCard')
  return (
    <FeatureLayout
      eyebrow="Workflows"
      title="Your process, your *statuses.*"
      body="Every project gets its own workflow. Add, rename, recolor and reorder statuses to match how your team works."
      chips={[
        { label: 'Unstarted', dot: '#8b8f98' },
        { label: 'Started', dot: '#f2c94c' },
        { label: 'Completed', dot: color.green },
        { label: 'Cancelled', dot: '#6b6f78' },
      ]}
    >
      <AppWindow
        src={shot('workflow')}
        width={W}
        url="orbit.yourcompany.com/tasks/projects/orbit/settings"
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 40, z: 1.4, x: center(card).x, y: card.y + 230 },
          { f: 120, z: 1.45, x: center(card).x, y: card.y + 330 },
        ]}
      >
        <Highlight box={card} from={44} pad={4} radius={14} dim={0.4} />
      </AppWindow>
    </FeatureLayout>
  )
}

/** Dark ↔ light split with a sweeping divider. */
export function Themes() {
  const frame = useCurrentFrame()
  const split = interpolate(frame, [20, 55, 80, 100], [100, 18, 18, 50], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: ease })
  return (
    <AbsoluteFill>
      <Background />
      <AbsoluteFill style={{ alignItems: 'center', paddingTop: 90, gap: 16 }}>
        <Eyebrow>Themes</Eyebrow>
        <Reveal text="Light or dark. *Your* call." size={80} align="center" from={4} />
      </AbsoluteFill>
      <AbsoluteFill style={{ alignItems: 'center', top: 330 }}>
        <div style={{ position: 'relative' }}>
          <AppWindow src={shot('docs-page')} width={1180} url="orbit.yourcompany.com/docs/welcome-to-orbit">
            <Img src={shot('docs-light')} style={{ position: 'absolute', inset: 0, width: SHOT_W, height: SHOT_H, clipPath: `inset(0 0 0 ${split}%)` }} />
          </AppWindow>
          <div style={{ position: 'absolute', top: 44, bottom: 0, left: `${split}%`, width: 3, marginLeft: -1.5, background: '#fff', boxShadow: `0 0 24px ${color.pink}` }}>
            <div style={{ position: 'absolute', top: '50%', left: -22, width: 46, height: 46, marginTop: -23, borderRadius: 99, background: '#fff', display: 'grid', placeItems: 'center', color: '#111', fontFamily: display, fontSize: 20, fontWeight: 700 }}>⇆</div>
          </div>
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}
