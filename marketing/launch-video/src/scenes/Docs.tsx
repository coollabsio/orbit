import { interpolate, useCurrentFrame } from 'remotion'
import { AppWindow, Highlight, ShotLayer } from '../components/AppWindow'
import { Cursor } from '../components/Cursor'
import { FeatureLayout } from '../components/FeatureLayout'
import { appFont, center, color, region, shot } from '../theme'

const W = 1240
const docsUrl = (path: string) => `orbit.yourcompany.com/docs/${path}`

export function DocsWrite() {
  const checklist = region('docs-page', 'checklist')
  const favorites = region('docs-page', 'favorites')
  return (
    <FeatureLayout
      eyebrow="Docs"
      title="Docs your team will *actually* read."
      body="A fast block editor with covers, emoji icons, nested pages and favorites, one click away from your tasks."
      chips={[{ label: 'Block editor', dot: color.pink }, { label: 'Nested pages', dot: color.violet }, { label: 'Covers & icons', dot: '#f2c94c' }, { label: 'Favorites', dot: color.green }]}
    >
      <AppWindow
        src={shot('docs-page')}
        width={W}
        url={docsUrl('welcome-to-orbit')}
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 40, z: 1.35, x: 960, y: 330 },
          { f: 95, z: 1.35, x: 960, y: 560 },
          { f: 150, z: 1.4, x: 960, y: 700 },
        ]}
      >
        <Highlight box={checklist} from={100} pad={10} radius={12} dim={0.4} />
        <Highlight box={favorites} from={12} to={44} pad={4} radius={10} dim={0.3} />
      </AppWindow>
    </FeatureLayout>
  )
}

export function DocsSlash() {
  const menu = region('docs-slash', 'slashMenu')
  const caret = region('docs-slash', 'caretLine')
  return (
    <FeatureLayout
      side="right"
      eyebrow="Editor"
      title="Type */* for anything."
      body="Headings, lists, toggles, quotes, tables, code, images and linked sub-pages, without leaving the keyboard."
      chips={[{ label: 'Markdown shortcuts', dot: color.violet }, { label: 'Drag handles', dot: color.pink }, { label: 'Sub-pages', dot: color.green }]}
    >
      <AppWindow
        src={shot('docs-slash')}
        width={W}
        url={docsUrl('design-principles')}
        camera={[
          { f: 0, z: 1.05, x: 840, y: 430 },
          { f: 34, z: 1.55, x: center(menu).x + 60, y: 560 },
          { f: 125, z: 1.62, x: center(menu).x + 60, y: 640 },
        ]}
      >
        <Highlight box={{ x: caret.x - 4, y: caret.y, w: 40, h: caret.h }} from={10} to={40} pad={2} radius={6} dim={0.3} />
        <Highlight box={menu} from={40} pad={4} radius={14} dim={0.45} />
        <Cursor from={46} path={[{ f: 46, x: 1300, y: 900 }, { f: 80, x: menu.x + 150, y: menu.y + 430 }, { f: 125, x: menu.x + 155, y: menu.y + 436 }]} clicks={[88]} />
      </AppWindow>
    </FeatureLayout>
  )
}

export function DocsRich() {
  const image = region('docs-rich', 'image')
  const table = region('docs-rich', 'table')
  const code = region('docs-rich', 'codeBlock')
  return (
    <FeatureLayout
      eyebrow="Rich content"
      title="Real docs, not just *notes.*"
      body="Diagrams, tables and code blocks live right next to the work they describe, and every page is searchable."
      chips={[{ label: 'Images', dot: color.pink }, { label: 'Tables', dot: color.violet }, { label: 'Code blocks', dot: color.green }, { label: 'Page links', dot: '#f2c94c' }]}
    >
      <AppWindow
        src={shot('docs-rich')}
        width={W}
        url={docsUrl('architecture-overview')}
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 30, z: 1.35, x: center(image).x, y: center(image).y },
          { f: 70, z: 1.35, x: center(table).x, y: center(table).y },
          { f: 110, z: 1.35, x: center(code).x, y: center(code).y },
          { f: 150, z: 1.4, x: center(code).x, y: center(code).y },
        ]}
      >
        <Highlight box={image} from={30} to={66} pad={6} radius={14} dim={0.4} />
        <Highlight box={table} from={70} to={106} pad={6} radius={10} dim={0.4} />
        <Highlight box={code} from={110} pad={6} radius={14} dim={0.4} />
      </AppWindow>
    </FeatureLayout>
  )
}

export function DocsSpaces() {
  const engineering = region('docs-tree', 'engineering')
  const company = region('docs-tree', 'company')
  const priv = region('docs-tree', 'private')
  return (
    <FeatureLayout
      side="right"
      eyebrow="Teamspaces"
      title="A home for every *team.*"
      body="Shared teamspaces, private pages for drafts and favorites for what you open daily. Drag pages anywhere."
      chips={[{ label: 'Teamspaces', dot: color.violet }, { label: 'Private pages', dot: color.pink }, { label: 'Drag to reorganize', dot: color.green }]}
    >
      <AppWindow
        src={shot('docs-tree')}
        width={W}
        url={docsUrl('rfcs')}
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 30, z: 1.75, x: 560, y: 320 },
          { f: 70, z: 1.75, x: 560, y: 560 },
          { f: 125, z: 1.75, x: 560, y: 720 },
        ]}
      >
        <Highlight box={engineering} from={28} to={62} pad={4} radius={10} dim={0.45} />
        <Highlight box={company} from={62} to={94} pad={4} radius={10} dim={0.45} />
        <Highlight box={priv} from={94} pad={4} radius={10} dim={0.45} />
      </AppWindow>
    </FeatureLayout>
  )
}

/** Notion import, end to end: paste a token, pick pages, watch it import, land on the result. */
export function NotionImport() {
  const frame = useCurrentFrame()
  const input = region('docs-import', 'tokenInput')
  const card = region('docs-import', 'connectCard')
  const chooser = region('docs-import-choose', 'chooser')
  const progress = region('docs-import-progress', 'progressCard')
  const result = region('docs-import-done', 'resultCard')
  const stats = region('docs-import-done', 'stats')
  const token = 'ntn_4f9c••••••••••••••••••••2b7e'
  const typed = token.slice(0, Math.max(0, Math.floor((frame - 44) / 1.1)))
  const scan = { x: input.x + 68, y: input.y + 60 }
  const [CHOOSE, PROGRESS, DONE] = [92, 160, 196]
  const tokenOpacity = interpolate(frame, [CHOOSE, CHOOSE + 8], [1, 0], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })
  return (
    <FeatureLayout
      eyebrow="Notion import"
      title="Leaving Notion? Bring *everything.*"
      body="Paste a Notion token, pick the pages you want and Orbit brings them over with their nesting, images and files."
      chips={[{ label: 'Pick what to import', dot: color.pink }, { label: 'Pages & sub-pages', dot: color.violet }, { label: 'Images & files', dot: color.green }]}
    >
      <AppWindow
        src={shot('docs-import')}
        width={W}
        url={docsUrl('import')}
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 30, z: 1.75, x: center(card).x, y: center(card).y + 40 },
          { f: CHOOSE - 4, z: 1.75, x: center(card).x, y: center(card).y + 40 },
          { f: CHOOSE + 14, z: 1.5, x: chooser.x + 420, y: chooser.y + 300 },
          { f: PROGRESS - 8, z: 1.55, x: chooser.x + 420, y: chooser.y + 380 },
          { f: PROGRESS + 8, z: 1.8, x: center(progress).x, y: center(progress).y + 60 },
          { f: DONE + 12, z: 1.55, x: center(result).x, y: center(result).y + 40 },
          { f: 240, z: 1.6, x: center(result).x, y: center(result).y + 40 },
        ]}
      >
        {frame >= 44 && tokenOpacity > 0 ? (
          <div
            style={{
              position: 'absolute',
              left: input.x + 1,
              top: input.y + 1,
              width: input.w - 2,
              height: input.h - 2,
              borderRadius: 7,
              background: '#141414',
              boxShadow: `0 0 0 1.5px ${color.pink}`,
              fontFamily: appFont,
              fontSize: 15,
              lineHeight: `${input.h - 2}px`,
              paddingLeft: 12,
              color: '#ececec',
              letterSpacing: '0.02em',
              opacity: tokenOpacity,
            }}
          >
            {typed}
            <span style={{ display: 'inline-block', width: 1.5, height: 16, marginLeft: 1, verticalAlign: 'middle', background: color.pink, opacity: Math.floor(frame / 8) % 2 ? 1 : 0.2 }} />
          </div>
        ) : null}
        <ShotLayer name="docs-import-choose" from={CHOOSE} />
        <ShotLayer name="docs-import-progress" from={PROGRESS} />
        <ShotLayer name="docs-import-done" from={DONE} />
        <Highlight box={stats} from={DONE + 16} pad={6} radius={12} dim={0.35} />
        <Cursor
          from={26}
          path={[
            { f: 26, x: 1250, y: 420 },
            { f: 40, x: input.x + 300, y: input.y + 18 },
            { f: 74, x: input.x + 300, y: input.y + 18 },
            { f: 86, x: scan.x, y: scan.y },
            { f: 118, x: 720, y: 560 },
            { f: 170, x: 724, y: 566 },
          ]}
          clicks={[42, 88, 124]}
        />
      </AppWindow>
    </FeatureLayout>
  )
}
