#!/usr/bin/env bun
// Fills the development workspace with a realistic team wiki (for screenshots) through the HTTP API:
// teamspaces, nested pages with rich BlockNote content, generated cover images, an in-page diagram and favorites.
// Safe to run again: teamspaces and pages that already exist (matched by name / title within the same space) are
// skipped, and nothing that already exists is deleted.
//
//   ORBIT_URL=http://127.0.0.1:18080 bun scripts/seed-demo-docs.ts
//   SEED_REFRESH=1 ...   also rewrites content/icon of the seeded pages that already exist (covers are kept)
//   SEED_REGEN_IMAGES=1  with SEED_REFRESH, uploads freshly generated covers and images again
//
// Images are rendered with `sharp`, resolved from marketing/launch-video/node_modules (or the current directory).

import { createRequire } from 'node:module'
import { join, resolve } from 'node:path'

const BASE = process.env.ORBIT_URL ?? `http://127.0.0.1:${process.env.ORBIT_DEV_API_PORT ?? 8080}`
// Development seed accounts (see `orbit seed`).
const LOGINS = { dev: 'test@example.com', member: 'member@example.com' }
const PASSWORD = 'password'
const WORKSPACE = process.env.ORBIT_WORKSPACE ?? 'Orbit Development'
const REFRESH = process.env.SEED_REFRESH === '1'
const REGEN_IMAGES = process.env.SEED_REGEN_IMAGES === '1'

type Who = 'dev' | 'member'

// ---------- sharp ----------
function loadSharp() {
  const roots = [resolve(import.meta.dir, '../marketing/launch-video/package.json'), join(process.cwd(), 'package.json')]
  for (const root of roots) {
    try {
      return createRequire(root)('sharp')
    } catch {}
  }
  throw new Error('sharp not found: run `bun install` in marketing/launch-video')
}
const sharp = loadSharp()

// ---------- images ----------
type Blob = { x: number; y: number; rx: number; ry: number; color: string; opacity: number }
const COVERS: Record<string, { base: [string, string]; blobs: Blob[]; rings?: { cx: number; cy: number } }> = {
  welcome: {
    base: ['#0b0f24', '#1a1036'],
    blobs: [
      { x: 260, y: 120, rx: 420, ry: 220, color: '#f2458f', opacity: 0.85 },
      { x: 820, y: 330, rx: 460, ry: 200, color: '#8b5cf6', opacity: 0.8 },
      { x: 1360, y: 90, rx: 380, ry: 200, color: '#ff9f6b', opacity: 0.55 },
      { x: 1250, y: 380, rx: 300, ry: 160, color: '#22d3ee', opacity: 0.35 },
    ],
    rings: { cx: 1180, cy: 200 },
  },
  architecture: {
    base: ['#07111f', '#0c1a33'],
    blobs: [
      { x: 180, y: 300, rx: 420, ry: 200, color: '#14b8a6', opacity: 0.75 },
      { x: 760, y: 80, rx: 480, ry: 190, color: '#3b82f6', opacity: 0.7 },
      { x: 1380, y: 300, rx: 420, ry: 210, color: '#8b5cf6', opacity: 0.75 },
    ],
    rings: { cx: 420, cy: 200 },
  },
  roadmap: {
    base: ['#0d0b24', '#190f33'],
    blobs: [
      { x: 1350, y: 110, rx: 440, ry: 210, color: '#8b5cf6', opacity: 0.85 },
      { x: 700, y: 360, rx: 520, ry: 190, color: '#f2458f', opacity: 0.7 },
      { x: 150, y: 90, rx: 380, ry: 190, color: '#14b8a6', opacity: 0.6 },
    ],
    rings: { cx: 1300, cy: 200 },
  },
  launch: {
    base: ['#140a1f', '#240c2c'],
    blobs: [
      { x: 1200, y: 330, rx: 520, ry: 210, color: '#f2458f', opacity: 0.85 },
      { x: 420, y: 100, rx: 460, ry: 200, color: '#fb923c', opacity: 0.6 },
      { x: 1500, y: 60, rx: 300, ry: 160, color: '#8b5cf6', opacity: 0.75 },
    ],
    rings: { cx: 300, cy: 220 },
  },
  timeoff: {
    base: ['#061a1c', '#0b1f2e'],
    blobs: [
      { x: 300, y: 300, rx: 480, ry: 200, color: '#14b8a6', opacity: 0.8 },
      { x: 1100, y: 90, rx: 520, ry: 190, color: '#38bdf8', opacity: 0.6 },
      { x: 1450, y: 340, rx: 320, ry: 170, color: '#f2458f', opacity: 0.45 },
    ],
  },
  rfc: {
    base: ['#0a0d1f', '#10162e'],
    blobs: [
      { x: 500, y: 200, rx: 520, ry: 170, color: '#6366f1', opacity: 0.75 },
      { x: 1250, y: 250, rx: 440, ry: 190, color: '#14b8a6', opacity: 0.55 },
    ],
    rings: { cx: 1350, cy: 200 },
  },
}

async function coverJpeg(name: keyof typeof COVERS): Promise<Uint8Array> {
  const c = COVERS[name]
  const W = 1600
  const H = 400
  const rings = c.rings
    ? [140, 230, 330, 450, 590]
        .map((r, i) => `<circle cx="${c.rings!.cx}" cy="${c.rings!.cy}" r="${r}" fill="none" stroke="#fff" stroke-opacity="${0.11 - i * 0.018}" stroke-width="1.5"/>`)
        .join('') + `<circle cx="${c.rings.cx + 230 * Math.cos(-0.6)}" cy="${c.rings.cy + 230 * Math.sin(-0.6)}" r="7" fill="#fff" fill-opacity="0.55"/>`
    : ''
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${c.base[0]}"/><stop offset="1" stop-color="${c.base[1]}"/></linearGradient>
    <filter id="blur" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="90"/></filter>
    <filter id="grain"><feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" stitchTiles="stitch"/><feColorMatrix type="saturate" values="0"/></filter>
    <linearGradient id="fade" x1="0" y1="0" x2="0" y2="1"><stop offset="0.55" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity="0.35"/></linearGradient>
  </defs>
  <rect width="${W}" height="${H}" fill="url(#bg)"/>
  <g filter="url(#blur)">${c.blobs.map((b) => `<ellipse cx="${b.x}" cy="${b.y}" rx="${b.rx}" ry="${b.ry}" fill="${b.color}" fill-opacity="${b.opacity}"/>`).join('')}</g>
  ${rings}
  <rect width="${W}" height="${H}" filter="url(#grain)" opacity="0.06"/>
  <rect width="${W}" height="${H}" fill="url(#fade)"/>
</svg>`
  return new Uint8Array(await sharp(Buffer.from(svg)).jpeg({ quality: 88, mozjpeg: true }).toBuffer())
}

async function architecturePng(): Promise<Uint8Array> {
  const W = 1600
  const H = 860
  const font = `font-family="Liberation Sans, DejaVu Sans, sans-serif"`
  const node = (x: number, y: number, w: number, h: number, accent: string, title: string, lines: string[], tag?: string) => `
    <g>
      <rect x="${x}" y="${y}" width="${w}" height="${h}" rx="18" fill="#141a33" stroke="${accent}" stroke-opacity="0.55" stroke-width="2"/>
      <rect x="${x}" y="${y}" width="6" height="${h}" rx="3" fill="${accent}"/>
      ${tag ? `<text x="${x + 30}" y="${y + 40}" ${font} font-size="17" font-weight="700" letter-spacing="2" fill="${accent}">${tag}</text>` : ''}
      <text x="${x + 30}" y="${y + (tag ? 76 : 50)}" ${font} font-size="30" font-weight="700" fill="#f4f5fb">${title}</text>
      ${lines.map((l, i) => `<text x="${x + 30}" y="${y + (tag ? 112 : 88) + i * 32}" ${font} font-size="21" fill="#a9afc7">${l}</text>`).join('')}
    </g>`
  const arrow = (x1: number, y1: number, x2: number, y2: number, label: string, color = '#8b93b8', lx?: number, ly?: number, dashed = false) => `
    <line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${color}" stroke-width="2.5" ${dashed ? 'stroke-dasharray="8 7"' : ''} marker-end="url(#head-${color.slice(1)})"/>
    <rect x="${(lx ?? (x1 + x2) / 2) - label.length * 5.4 - 12}" y="${(ly ?? (y1 + y2) / 2) - 19}" width="${label.length * 10.8 + 24}" height="32" rx="16" fill="#0b0f22" stroke="${color}" stroke-opacity="0.4"/>
    <text x="${lx ?? (x1 + x2) / 2}" y="${(ly ?? (y1 + y2) / 2) + 4}" text-anchor="middle" ${font} font-size="18" fill="${color}">${label}</text>`
  const heads = ['8b93b8', 'f2458f', '14b8a6', '8b5cf6']
    .map((c) => `<marker id="head-${c}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="#${c}"/></marker>`)
    .join('')
  const dots = Array.from({ length: 40 }, (_, i) => Array.from({ length: 22 }, (_, j) => `<circle cx="${20 + i * 40}" cy="${20 + j * 40}" r="1.3" fill="#fff" fill-opacity="0.07"/>`).join('')).join('')
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
  <defs>${heads}
    <radialGradient id="glow" cx="0.5" cy="0.45" r="0.6"><stop offset="0" stop-color="#8b5cf6" stop-opacity="0.22"/><stop offset="1" stop-color="#8b5cf6" stop-opacity="0"/></radialGradient>
  </defs>
  <rect width="${W}" height="${H}" rx="28" fill="#0b0f22"/>
  <rect width="${W}" height="${H}" rx="28" fill="url(#glow)"/>
  ${dots}
  <text x="60" y="78" ${font} font-size="34" font-weight="700" fill="#f4f5fb">Orbit — system overview</text>
  <text x="60" y="114" ${font} font-size="20" fill="#7d84a3">One binary, one database. Everything else is optional.</text>

  ${node(60, 190, 400, 250, '#f2458f', 'Web app', ['React 19 · Vite', 'BlockNote editor', 'TanStack Query cache'], 'CLIENT')}
  ${node(600, 190, 420, 250, '#8b5cf6', 'Orbit API', ['Rust · axum · tokio', 'OpenAPI contract v1', 'Auth, audit log, jobs'], 'SERVER')}
  ${node(1160, 150, 380, 160, '#14b8a6', 'SQLite (WAL)', ['Tasks, pages, audit'])}
  ${node(1160, 350, 380, 160, '#14b8a6', 'Blob store', ['Attachments, page files'])}
  ${node(600, 580, 420, 200, '#fb923c', 'Integrations', ['GitHub · Discord · SMTP', 'Notion import worker'], 'WORKERS')}
  ${node(60, 580, 400, 200, '#38bdf8', 'Realtime', ['Server-sent events', 'per-workspace channel'], 'PUSH')}

  ${arrow(460, 280, 598, 280, 'REST / JSON', '#f2458f', 529, 250)}
  ${arrow(1020, 250, 1158, 230, 'sqlx', '#14b8a6', 1089, 214)}
  ${arrow(1020, 360, 1158, 420, 'dedup', '#14b8a6', 1089, 420)}
  ${arrow(810, 440, 810, 578, 'job queue', '#8b93b8', 810, 512)}
  ${arrow(600, 400, 400, 578, 'events', '#8b5cf6', 470, 500, true)}
  ${arrow(260, 578, 260, 442, 'invalidate', '#38bdf8', 260, 512, true)}
</svg>`
  return new Uint8Array(await sharp(Buffer.from(svg)).png({ compressionLevel: 9 }).toBuffer())
}

// `SEED_PREVIEW_IMAGES=/tmp/dir` only writes the generated images there (no API calls).
if (process.env.SEED_PREVIEW_IMAGES) {
  const dir = process.env.SEED_PREVIEW_IMAGES
  for (const name of Object.keys(COVERS)) await Bun.write(join(dir, `cover-${name}.jpg`), await coverJpeg(name))
  await Bun.write(join(dir, 'architecture.png'), await architecturePng())
  console.log(`images written to ${dir}`)
  process.exit(0)
}

// ---------- BlockNote content DSL ----------
type Inline = Record<string, unknown>
type Block = Record<string, unknown>
type Ctx = { ref: (title: string) => string; image: (key: string) => string }

/** Mini markup: **bold**, *italic*, `code`, [text](https://…) and [text](page:Title) for in-app page links. */
function inline(ctx: Ctx, source: string): Inline[] {
  const out: Inline[] = []
  const re = /(\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`|\[[^\]]+\]\([^)]+\))/g
  let last = 0
  for (const match of source.matchAll(re)) {
    const index = match.index ?? 0
    if (index > last) out.push({ type: 'text', text: source.slice(last, index), styles: {} })
    const token = match[0]
    if (token.startsWith('**')) out.push({ type: 'text', text: token.slice(2, -2), styles: { bold: true } })
    else if (token.startsWith('`')) out.push({ type: 'text', text: token.slice(1, -1), styles: { code: true } })
    else if (token.startsWith('*')) out.push({ type: 'text', text: token.slice(1, -1), styles: { italic: true } })
    else {
      const [, text, target] = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token)!
      const href = target.startsWith('page:') ? `/docs/${ctx.ref(target.slice(5))}` : target
      out.push({ type: 'link', href, content: [{ type: 'text', text, styles: {} }] })
    }
    last = index + token.length
  }
  if (last < source.length) out.push({ type: 'text', text: source.slice(last), styles: {} })
  return out
}

function dsl(ctx: Ctx) {
  const block = (type: string, text: string | null, props: Record<string, unknown> = {}, children: Block[] = []): Block => ({
    type,
    props,
    ...(text === null ? {} : { content: inline(ctx, text) }),
    children,
  })
  return {
    p: (text: string) => block('paragraph', text),
    empty: () => block('paragraph', ''),
    h1: (text: string) => block('heading', text, { level: 1 }),
    h2: (text: string) => block('heading', text, { level: 2 }),
    h3: (text: string) => block('heading', text, { level: 3 }),
    ul: (text: string, children: Block[] = []) => block('bulletListItem', text, {}, children),
    ol: (text: string, children: Block[] = []) => block('numberedListItem', text, {}, children),
    todo: (text: string, checked = false) => block('checkListItem', text, { checked }),
    toggle: (text: string, children: Block[]) => block('toggleListItem', text, {}, children),
    quote: (text: string) => block('quote', text),
    /** Emoji-led note. Rendered as a quote: tinted paragraphs are too loud in the dark theme. */
    callout: (text: string) => block('quote', text),
    hr: (): Block => ({ type: 'divider', props: {}, children: [] }),
    code: (language: string, text: string): Block => ({ type: 'codeBlock', props: { language }, content: [{ type: 'text', text, styles: {} }], children: [] }),
    table: (rows: string[][], columnWidths?: number[]): Block => ({
      type: 'table',
      props: {},
      content: {
        type: 'tableContent',
        columnWidths: columnWidths ?? rows[0].map(() => undefined),
        headerRows: 1,
        rows: rows.map((cells) => ({ cells: cells.map((cell) => ({ type: 'tableCell', props: {}, content: inline(ctx, cell) })) })),
      },
      children: [],
    }),
    image: (key: string, caption: string, name: string, previewWidth = 740): Block => ({
      type: 'image',
      props: { url: ctx.image(key), caption, name, showPreview: true, previewWidth, textAlignment: 'center' },
      children: [],
    }),
    page: (title: string): Block => ({ type: 'page', props: { pageId: ctx.ref(title) }, children: [] }),
  }
}
type D = ReturnType<typeof dsl>

// ---------- the wiki ----------
type Space = 'default' | 'private' | 'Engineering' | 'Product' | 'Company'
type DemoPage = {
  space: Space
  title: string
  icon: string
  parent?: string
  /** Who creates the page (and writes it unless `editor` is set). */
  who?: Who
  /** Who writes the content (shows up as `updated_by`). */
  editor?: Who
  cover?: keyof typeof COVERS
  coverPosition?: string
  images?: Record<string, () => Promise<Uint8Array>>
  body?: (d: D) => Block[]
}

const TEAMSPACES = [
  { name: 'Engineering', icon: '🛠️' },
  { name: 'Product', icon: '🚀' },
  { name: 'Company', icon: '🏢' },
]

const PAGES: DemoPage[] = [
  // ----- default teamspace -----
  {
    space: 'default', title: 'Start here', icon: '📌',
    body: (d) => [
      d.p('New to the workspace? These three pages cover 90% of what you need in your first week.'),
      d.page('Welcome to Orbit'),
      d.page('Architecture overview'),
      d.page('Q4 2026 roadmap'),
      d.p('Everything else lives in the **Engineering**, **Product** and **Company** teamspaces. Press `Ctrl K` to search any page or task.'),
    ],
  },

  // ----- Company -----
  {
    space: 'Company', title: 'Welcome to Orbit', icon: '👋', cover: 'welcome', coverPosition: '50,40',
    body: (d) => [
      d.p("Hey, and welcome aboard! 🎉 We're a small team building **Orbit** — a self-hosted workspace where tasks and docs live side by side. This page is your map for the first two weeks. Skim it now, come back to it often."),
      d.callout('💡 Tip: star this page with the ☆ in the header so it stays pinned in your sidebar under **Favorites**.'),
      d.h2('What we believe'),
      d.ul('**Own your data.** Orbit runs on one binary and one SQLite file. No vendor can take your roadmap hostage.'),
      d.ul('**Fast is a feature.** Every interaction should feel instant — we budget *100 ms* for anything a user clicks.'),
      d.ul('**Write it down.** Decisions live in docs, not in DMs. If it is not in Orbit, it did not happen.'),
      d.ul('**Ship small, ship often.** We release every Tuesday; see the [release process](page:Release process).'),
      d.h2('Your first week'),
      d.todo('Get your accounts set up — see [Tools & accounts](page:Tools & accounts)', true),
      d.todo('Run Orbit locally with `just dev` and create your first task', true),
      d.todo('Read the [Architecture overview](page:Architecture overview)', false),
      d.todo('Pair with someone on a *good first issue* from the board', false),
      d.todo('Join the Tuesday demo and say hi 👋', false),
      d.h2('Who to ask'),
      d.table([
        ['Topic', 'Person', 'Where'],
        ['Product & roadmap', 'Andras', '#product'],
        ['Backend & infra', 'Mia', '#engineering'],
        ['Design & brand', 'Jonas', '#design'],
        ['Hiring & people', 'Priya', '#people'],
      ]),
      d.h2('How we communicate'),
      d.p('Async first. Long-form thinking goes into a page (use an [RFC](page:RFCs) for anything that changes architecture), quick questions go to chat, and anything with a deadline becomes a **task** with an owner and a due date.'),
      d.quote('"The best time to write the doc was before the meeting. The second best time is right after it." — our unofficial motto'),
      d.h3('Core hours'),
      d.p('We overlap **14:00–17:00 CET**. Outside of that, work when you do your best work. Put focus time and vacations in the shared calendar and in [Time off policy](page:Time off policy).'),
    ],
  },
  {
    space: 'Company', parent: 'Welcome to Orbit', title: 'How we work', icon: '🧭',
    body: (d) => [
      d.p('A lightweight operating rhythm that keeps a small team moving without meetings eating the week.'),
      d.h2('Weekly rhythm'),
      d.table([
        ['Day', 'Ritual', 'Length'],
        ['Monday', 'Planning: pick the week from *My week*', '30 min'],
        ['Tuesday', 'Release + demo', '45 min'],
        ['Thursday', 'Customer call review', '30 min'],
        ['Friday', 'Written weekly update', 'async'],
      ]),
      d.h2('Tasks'),
      d.ul('Every task has **one** owner. Multiple assignees are fine, but one person drives it.'),
      d.ul('Use priorities honestly: *Urgent* means someone is blocked right now.'),
      d.ul('Link duplicates instead of closing them silently — the history matters.'),
    ],
  },
  {
    space: 'Company', parent: 'Welcome to Orbit', title: 'Tools & accounts', icon: '🧰',
    body: (d) => [
      d.p('Ask in `#people` if any of these invites are missing.'),
      d.todo('Orbit workspace (you are here ✅)', true),
      d.todo('GitHub organization `coollabsio`', true),
      d.todo('Discord — join `#engineering` and `#launch`', false),
      d.todo('1Password vault “Orbit team”', false),
      d.todo('Figma: *Orbit Design System* file', false),
    ],
  },
  { space: 'Company', title: 'Meeting notes', icon: '🗓️',
    body: (d) => [
      d.p('Notes from recurring meetings, newest first. Create a sub-page per meeting and title it `Meeting — Mon DD, YYYY`.'),
      d.page('Weekly sync — Sep 22, 2026'),
      d.page('Launch go/no-go — Sep 18, 2026'),
      d.page('Weekly sync — Sep 15, 2026'),
    ],
  },
  {
    space: 'Company', parent: 'Meeting notes', title: 'Weekly sync — Sep 22, 2026', icon: '📝', who: 'member',
    body: (d) => [
      d.p('**Attendees:** Andras, Mia, Jonas, Priya · **Facilitator:** Mia'),
      d.h2('Updates'),
      d.ul('Docs beta is live for all internal workspaces — 214 pages created in the first week.'),
      d.ul('Realtime sync is behind a flag; p95 fan-out latency is **38 ms** on the staging box.'),
      d.ul('Two design partners asked for Notion import. It is already built — we need a guide.'),
      d.h2('Decisions'),
      d.ul('Launch date stays **October 14**. Go/no-go on October 9.'),
      d.ul('Offline mode moves to Q1; see [RFC 013: Offline mode](page:RFC 013: Offline mode).'),
      d.h2('Action items'),
      d.todo('Mia: finish the realtime presence avatars', false),
      d.todo('Jonas: record the 60-second launch video', false),
      d.todo('Priya: book three more customer interviews', true),
      d.todo('Andras: draft the Hacker News post', false),
    ],
  },
  {
    space: 'Company', parent: 'Meeting notes', title: 'Launch go/no-go — Sep 18, 2026', icon: '🚦',
    body: (d) => [
      d.p('Pre-read: [Launch plan: Orbit 1.0](page:Launch plan: Orbit 1.0). Result: **go**, with two conditions.'),
      d.todo('Backups verified on a fresh VPS restore', true),
      d.todo('Upgrade path from 0.9 tested with real data', true),
      d.todo('Status page and on-call rotation in place', false),
    ],
  },
  {
    space: 'Company', parent: 'Meeting notes', title: 'Weekly sync — Sep 15, 2026', icon: '📝', who: 'member',
    body: (d) => [
      d.p('**Attendees:** Andras, Mia, Priya'),
      d.ul('Bulk actions shipped. Feedback: people want *Move to project* in the bar.'),
      d.ul('Search now covers page bodies, not only titles.'),
      d.todo('Mia: profile the task list with 5,000 rows', true),
    ],
  },
  {
    space: 'Company', title: 'Time off policy', icon: '🌴', cover: 'timeoff',
    body: (d) => [
      d.p('Rest is part of the job. We would rather you take a real break than slowly burn out.'),
      d.h2('The short version'),
      d.ul('**30 days** of paid time off per year, plus your local public holidays.'),
      d.ul('Take at least **two consecutive weeks** once a year. We check.'),
      d.ul('Sick days are unlimited and need no doctor’s note for the first three days.'),
      d.h2('How to book it'),
      d.ol('Add it to the shared calendar as *Out of office*.'),
      d.ol('Post the dates in `#people` at least two weeks ahead for anything longer than 3 days.'),
      d.ol('Hand over anything urgent: reassign tasks due while you are away.'),
      d.callout('🌴 While you are off, you are off. Log out of chat. Nobody expects a reply.'),
    ],
  },

  // ----- Engineering -----
  {
    space: 'Engineering', title: 'Architecture overview', icon: '🏗️', cover: 'architecture', coverPosition: '50,50',
    images: { diagram: architecturePng },
    body: (d) => [
      d.p('Orbit is a **single Rust binary** that serves the API and the web app, backed by one **SQLite** database in WAL mode. This page explains how the pieces fit together and where to look when something breaks.'),
      d.image('diagram', 'Request flow: the web app talks REST to the API, which pushes change events back over SSE.', 'orbit-architecture.png', 560),
      d.h2('Components'),
      d.table([
        ['Component', 'Stack', 'Owner', 'Notes'],
        ['Web app', 'React 19 · Vite', 'Jonas', 'Editor is lazy-loaded'],
        ['API', 'Rust · axum · sqlx', 'Mia', 'Contract `orbit-api-v1`'],
        ['Database', 'SQLite (WAL)', 'Mia', 'Nightly backups'],
        ['Realtime', 'Server-sent events', 'Andras', 'One channel per workspace'],
      ]),
      d.h2('Adding an endpoint'),
      d.code('rust', `pub async fn create_page(
    State(app): State<AppState>,
    member: WorkspaceMember,
    Json(body): Json<CreatePageBody>,
) -> Result<Json<Page>, ApiError> {
    let page = app.pages().create(&member, body).await?;
    app.realtime().publish(member.workspace_id, Change::Page(page.id));
    Ok(Json(page))
}`),
      d.callout('⚠️ Never call `publish` inside the transaction — clients may refetch before the commit lands.'),
      d.p('Handlers are plain async functions. Keep them thin: validation in the body type, logic in a repository.'),
      d.h2('Request lifecycle'),
      d.ol('The browser sends a request with the session cookie and the `x-orbit-contract` header.'),
      d.ol('Middleware checks origin, contract version and rate limits, then opens a transaction.'),
      d.ol('The handler writes the change **and** an audit event in the same transaction.'),
      d.ol('After commit, a change event is published to the workspace channel; open tabs refetch.'),
      d.h2('Local setup checklist'),
      d.todo('Install Rust stable and Bun', true),
      d.todo('Run `just dev` (API on :8080, web on :5173)', true),
      d.todo('Seed demo data with `orbit seed`', false),
      d.h3('Further reading'),
      d.ul('[RFC 012: Realtime sync](page:RFC 012: Realtime sync) — how change events are fanned out'),
      d.ul('[Testing guide](page:Testing guide) — what to test where'),
      d.ul('[SQLite WAL mode](https://www.sqlite.org/wal.html) — why readers never block writers'),
    ],
  },
  {
    space: 'Engineering', title: 'Release process', icon: '🚢',
    body: (d) => [
      d.p('We ship every **Tuesday at 11:00 CET**. A release is boring on purpose: if any step feels exciting, stop and ask.'),
      d.h2('Steps'),
      d.ol('Freeze `main` in `#engineering` and wait for the last CI run to go green.'),
      d.ol('Run `just release-notes` and edit the draft: user-facing language, no ticket IDs.'),
      d.ol('Tag the release: `git tag v1.0.3 && git push --tags`. CI builds the images.'),
      d.ol('Deploy to **staging**, click through the smoke checklist below.'),
      d.ol('Promote to production and watch error rates for 30 minutes.'),
      d.ol('Post the notes in `#launch` and on the changelog.'),
      d.h2('Smoke checklist'),
      d.todo('Log in with a password and with a magic link', true),
      d.todo('Create, move and complete a task on the board', true),
      d.todo('Edit a doc in two tabs — the second tab updates', true),
      d.todo('Upload a cover image to a page', false),
      d.todo('Restore a page from the trash', false),
      d.h2('Rolling back'),
      d.p('Every release keeps the previous image. Roll back with:'),
      d.code('bash', `orbit deploy --image ghcr.io/coollabsio/orbit:v1.0.2
orbit migrate --check   # migrations are forward-compatible for one release`),
      d.quote('If you are unsure whether to roll back, roll back. We can always ship again in an hour.'),
    ],
  },
  {
    space: 'Engineering', title: 'Testing guide', icon: '🧪', who: 'member',
    body: (d) => [
      d.p('Tests should give us the confidence to ship on a Tuesday morning without a manual QA pass.'),
      d.h2('What goes where'),
      d.table([
        ['Layer', 'Tool', 'Run with'],
        ['Rust units & repositories', 'cargo test', '`just test-api`'],
        ['API contract', 'OpenAPI snapshot', '`just test-contract`'],
        ['React components', 'Vitest + Testing Library', '`bun test`'],
        ['End-to-end', 'Playwright', '`just e2e`'],
      ]),
      d.h2('Rules of thumb'),
      d.ul('Test behavior, not implementation: query by role and label, never by class name.'),
      d.ul('Every bug fix starts with a failing test.'),
      d.ul('A flaky test is a bug. Quarantine it within the day, fix it within the week.'),
      d.code('typescript', `test('moves a card to Done', async () => {
  render(<Board />)
  await user.drag(screen.getByText('Ship docs'), screen.getByRole('list', { name: 'Done' }))
  expect(await screen.findByRole('status')).toHaveTextContent('Moved to Done')
})`),
    ],
  },
  {
    space: 'Engineering', title: 'RFCs', icon: '📐',
    body: (d) => [
      d.p('An RFC is how we make decisions that are expensive to undo. Write one when a change touches the data model, the API contract or more than one team.'),
      d.h2('Process'),
      d.ol('Copy the template, fill in **Problem**, **Proposal** and **Alternatives**.'),
      d.ol('Share it in `#engineering`. Comments stay open for five working days.'),
      d.ol('The owner records the decision at the top: *Accepted*, *Rejected* or *Postponed*.'),
      d.h2('Index'),
      d.page('RFC 012: Realtime sync'),
      d.page('RFC 013: Offline mode'),
      d.page('RFC 014: Page version history'),
    ],
  },
  {
    space: 'Engineering', parent: 'RFCs', title: 'RFC 012: Realtime sync', icon: '⚡', cover: 'rfc',
    body: (d) => [
      d.callout('✅ Status: **Accepted** · Owner: Mia · Last updated Sep 12, 2026'),
      d.h2('Problem'),
      d.p('Two people editing the same board see stale data until they refresh. Users report moving a card that a teammate already closed.'),
      d.h2('Proposal'),
      d.p('Publish a small change event after every committed write and let clients invalidate the matching queries. No document merging on the server — just *“something changed, refetch”*.'),
      d.code('json', `{ "type": "task.updated", "workspace_id": "01a0…", "id": "01a1…", "version": 42 }`),
      d.h2('Alternatives considered'),
      d.ul('**CRDTs everywhere** — great for text, overkill for task fields, heavy on the client.'),
      d.ul('**Polling every 5 s** — simple, but wasteful and still feels laggy.'),
      d.h2('Rollout'),
      d.todo('Events for tasks and comments', true),
      d.todo('Events for pages and favorites', true),
      d.todo('Presence avatars on boards', false),
    ],
  },
  {
    space: 'Engineering', parent: 'RFCs', title: 'RFC 013: Offline mode', icon: '📴', who: 'member',
    body: (d) => [
      d.callout('⏸️ Status: **Postponed to Q1 2027** · Owner: Priya'),
      d.h2('Problem'),
      d.p('People on trains and planes want to keep writing. Today the editor shows *Save failed* and waits.'),
      d.h2('Proposal'),
      d.p('Queue page edits in IndexedDB, replay them in order when the connection returns, and surface conflicts with the existing *Reload / Overwrite* banner.'),
      d.h2('Open questions'),
      d.ul('How long do we keep a queued edit before asking the user?'),
      d.ul('Do private pages sync to every device, or only the ones opened recently?'),
    ],
  },
  {
    space: 'Engineering', parent: 'RFCs', title: 'RFC 014: Page version history', icon: '🕰️',
    body: (d) => [
      d.callout('📝 Status: **Draft** · Owner: Andras'),
      d.p('Keep a snapshot of a page every 10 minutes of active editing and on every restore, with a side-by-side diff view.'),
    ],
  },
  {
    space: 'Engineering', parent: 'Release process', title: 'Incident runbook', icon: '🧯',
    body: (d) => [
      d.p('Something is on fire. Breathe. Follow the list.'),
      d.ol('Acknowledge the alert and post in `#incidents`: what you see, since when.'),
      d.ol('Check the health endpoint: `curl -s https://orbit.example.com/api/health`.'),
      d.ol('If the database is locked, restart the service — WAL recovery takes seconds.'),
      d.ol('Write the timeline into a new page under **Meeting notes** within 48 hours.'),
    ],
  },

  // ----- Product -----
  {
    space: 'Product', title: 'Q4 2026 roadmap', icon: '🗺️', cover: 'roadmap', coverPosition: '50,45',
    body: (d) => [
      d.p('Our goal for Q4: **make Orbit the place a small team can run on — tasks and docs, self-hosted, in one tab.** Three themes, in priority order. Each links to the tasks on the board.'),
      d.callout('🎯 North star metric: weekly active teams that create both a task **and** a page. Target: **500** by Dec 31.'),
      d.h2('1 · Docs that feel like Notion'),
      d.p('Docs shipped in beta. Q4 is about polish and trust — nobody moves their wiki to a tool that loses a paragraph.'),
      d.todo('Nested pages, teamspaces and private pages', true),
      d.todo('Covers, icons and image uploads', true),
      d.todo('Favorites in the sidebar', true),
      d.todo('Notion import with page links preserved', true),
      d.todo('Page version history', false),
      d.todo('Comments on blocks', false),
      d.page('RFC 014: Page version history'),
      d.h2('2 · Realtime collaboration'),
      d.ul('Boards and pages update live when a teammate edits — no refresh.'),
      d.ul('Presence avatars on boards and in the page header.'),
      d.ul('Conflict banner with *Reload* / *Overwrite* stays as the safety net.'),
      d.page('RFC 012: Realtime sync'),
      d.h2('3 · Launch Orbit 1.0'),
      d.ul('One-command install on any VPS, with Coolify as a first-class option.'),
      d.ul('Public docs site and a 60-second launch video.'),
      d.ul('See the full plan in [Launch plan: Orbit 1.0](page:Launch plan: Orbit 1.0).'),
      d.h2('Not this quarter'),
      d.ul('*Offline mode* — postponed, see [RFC 013](page:RFC 013: Offline mode).'),
      d.ul('*Native mobile apps* — the responsive web app covers the core flows.'),
      d.table([
        ['Theme', 'Owner', 'Confidence', 'Ships'],
        ['Docs polish', 'Jonas', '🟢 High', 'Oct'],
        ['Realtime', 'Mia', '🟡 Medium', 'Nov'],
        ['1.0 launch', 'Andras', '🟢 High', 'Oct 14'],
      ]),
    ],
  },
  {
    space: 'Product', title: 'Launch plan: Orbit 1.0', icon: '📣', cover: 'launch', editor: 'member',
    body: (d) => [
      d.p('**Launch day: Tuesday, October 14, 2026.** One message everywhere: *Orbit is a self-hosted Linear and Notion alternative — tasks and docs in one place, on your own server.*'),
      d.quote('"We moved 40 people off three SaaS tools in an afternoon. Our data never left our server." — Lena, CTO at Northwind Labs'),
      d.h2('Channels'),
      d.table([
        ['Channel', 'Owner', 'Asset', 'When'],
        ['Hacker News', 'Andras', 'Show HN post', '15:00 CET'],
        ['X / Bluesky', 'Jonas', '60s video + thread', '15:05 CET'],
        ['Coolify newsletter', 'Priya', 'Feature story', 'Oct 15'],
        ['Product Hunt', 'Mia', 'Gallery + maker comment', 'Oct 21'],
      ]),
      d.h2('Launch checklist'),
      d.todo('Landing page copy final', true),
      d.todo('Pricing: free & open source, paid support tier', true),
      d.todo('Install script tested on Ubuntu, Debian and Hetzner images', true),
      d.todo('Launch video (60 s) exported in 16:9 and 9:16', false),
      d.todo('Docs site: install, upgrade, backup, Notion import', false),
      d.todo('Status page live', false),
      d.h2('Risks'),
      d.ul('**Traffic spike** on the demo instance — put it behind a CDN and cap signups.'),
      d.ul('**Install failures** on exotic distros — have someone on `#support` all day.'),
    ],
  },
  {
    space: 'Product', title: 'Customer interviews', icon: '🎯',
    body: (d) => [
      d.p('One page per interview. Tag insights with **[docs]**, **[tasks]** or **[self-hosting]** so we can count them later.'),
      d.h2('Recurring themes (12 interviews)'),
      d.ul('9 of 12 pay for both Linear and Notion and want **one bill, one login**.'),
      d.ul('7 of 12 have a compliance reason to self-host.'),
      d.ul('Everyone asks about **Notion import** in the first five minutes.'),
      d.page('Interview: Northwind Labs'),
      d.page('Interview: Tidal Studio'),
    ],
  },
  {
    space: 'Product', parent: 'Customer interviews', title: 'Interview: Northwind Labs', icon: '🏭', who: 'member',
    body: (d) => [
      d.p('**Who:** Lena (CTO), 40-person hardware startup · **Date:** Sep 10, 2026 · **Interviewer:** Priya'),
      d.h2('Highlights'),
      d.quote('"Our specs are in Notion, our tickets in Linear, and nobody can find anything. I want one search box." '),
      d.ul('[self-hosting] Customer contracts require data to stay in the EU, on their own hardware.'),
      d.ul('[docs] Heavy use of nested pages — some trees are 6 levels deep.'),
      d.ul('[tasks] Loves keyboard shortcuts, hates custom fields.'),
      d.h2('Follow-ups'),
      d.todo('Send the Notion import guide', true),
      d.todo('Invite to the 1.0 beta', false),
    ],
  },
  {
    space: 'Product', parent: 'Customer interviews', title: 'Interview: Tidal Studio', icon: '🌊',
    body: (d) => [
      d.p('**Who:** Marco (founder), 8-person design agency · **Date:** Sep 17, 2026'),
      d.ul('[docs] Wants client-facing pages with covers that look *“as good as Notion”*.'),
      d.ul('[tasks] Uses the board for every client project; roadmap view for retainers.'),
    ],
  },
  {
    space: 'Product', title: 'Design principles', icon: '🎨', cover: 'welcome', coverPosition: '50,70',
    body: (d) => [
      d.p('How we make product calls when there is no spec to lean on.'),
      d.h2('Principles'),
      d.ol('**Calm by default.** Color is for meaning, not decoration.'),
      d.ol('**Keyboard first, mouse friendly.** Every action has a shortcut *and* a visible button.'),
      d.ol('**No dead ends.** Every empty state tells you what to do next.'),
      d.ol('**Fast beats clever.** If an animation delays the result, cut it.'),
      d.ol('**One way to do it.** Two buttons for the same thing means we have not decided yet.'),
      d.h2('Checklist for every new screen'),
      d.todo('Works with the keyboard alone', true),
      d.todo('Readable in light and dark theme', true),
      d.todo('Empty, loading and error states designed', false),
      d.quote('Good design is as little design as possible. — Dieter Rams'),
    ],
  },

  // ----- Private -----
  {
    space: 'private', title: 'My scratchpad', icon: '✍️',
    body: (d) => [
      d.p('Only I can see this page. 🔒'),
      d.h3('Ideas'),
      d.ul('Slash command to turn a checklist item into a task?'),
      d.ul('“Copy link to block” in the drag handle menu'),
      d.h3('Today'),
      d.todo('Review Mia’s realtime PR', true),
      d.todo('Answer Northwind about SSO', false),
      d.todo('Write the Show HN draft', false),
    ],
  },
  {
    space: 'private', title: 'Reading list', icon: '📚',
    body: (d) => [
      d.ul('[Local-first software](https://www.inkandswitch.com/local-first/) — Ink & Switch'),
      d.ul('[Consider SQLite](https://blog.wesleyac.com/posts/consider-sqlite)'),
      d.ul('*Shape Up* — Ryan Singer'),
    ],
  },
]

const FAVORITES = ['Q4 2026 roadmap', 'Architecture overview', 'Welcome to Orbit']

// ---------- API ----------
let cookie = ''
const headers = () => ({ origin: BASE, 'x-orbit-contract': 'orbit-api-v1', cookie })
async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(`${BASE}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...headers() },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const setCookie = response.headers.get('set-cookie')
  if (setCookie) cookie = setCookie.split(';')[0]
  if (!response.ok) throw new Error(`${method} ${path} → ${response.status}: ${await response.text()}`)
  return (response.status === 204 ? undefined : await response.json()) as T
}

async function upload(path: string, name: string, bytes: Uint8Array): Promise<{ url: string }> {
  const form = new FormData()
  form.append('file', new Blob([bytes], { type: name.endsWith('.jpg') ? 'image/jpeg' : 'image/png' }), name)
  const response = await fetch(`${BASE}${path}`, { method: 'POST', headers: headers(), body: form })
  if (!response.ok) throw new Error(`POST ${path} → ${response.status}: ${await response.text()}`)
  return (await response.json()) as { url: string }
}

const cookies = {} as Record<Who, string>
for (const who of ['member', 'dev'] as const) {
  cookie = ''
  await api('POST', '/api/v1/auth/login', { email: LOGINS[who], password: PASSWORD })
  cookies[who] = cookie
}
const as = (who: Who) => {
  cookie = cookies[who]
}

const workspaces = await api<{ id: string; name: string }[]>('GET', '/api/v1/workspaces')
const workspace = workspaces.find((item) => item.name === WORKSPACE) ?? workspaces[0]
const ws = `/api/v1/workspaces/${workspace.id}`
console.log(`workspace: ${workspace.name}`)

type Teamspace = { id: string; name: string; icon: string | null; is_default: boolean; version: number }
const teamspaces = (await api<{ items: Teamspace[] }>('GET', `${ws}/teamspaces`)).items
const spaceIds = new Map<Space, string>()
spaceIds.set('default', teamspaces.find((item) => item.is_default)!.id)
for (const def of TEAMSPACES) {
  let found = teamspaces.find((item) => item.name === def.name)
  if (!found) {
    found = await api<Teamspace>('POST', `${ws}/teamspaces`, def)
    console.log(`+ teamspace ${def.icon} ${def.name}`)
  } else if (!found.icon) {
    await api('PATCH', `${ws}/teamspaces/${found.id}`, { expected_version: found.version, icon: def.icon })
  }
  spaceIds.set(def.name as Space, found.id)
}

type Summary = { id: string; parent_id: string | null; teamspace_id: string | null; private: boolean; title: string }
type Page = Summary & { version: number; cover_url: string | null; content: unknown[] }
const spaceOf = (summary: Summary): string => (summary.private ? 'private' : summary.teamspace_id!)
const spaceKey = (space: Space) => (space === 'private' ? 'private' : spaceIds.get(space)!)
const titleKey = (space: string, title: string) => `${space}|${title}`

as('dev')
const existing = new Map((await api<{ items: Summary[] }>('GET', `${ws}/pages`)).items.map((item) => [titleKey(spaceOf(item), item.title), item.id]))
const ids = new Map<string, string>()
const fresh = new Set<string>()
for (const def of PAGES) {
  const key = titleKey(spaceKey(def.space), def.title)
  const found = existing.get(key)
  if (found) {
    ids.set(def.title, found)
    continue
  }
  // Private pages belong to the dev account (the one the screenshots log in with).
  as(def.space === 'private' ? 'dev' : (def.who ?? 'dev'))
  const body: Record<string, unknown> = { title: def.title, icon: def.icon }
  if (def.parent) body.parent_id = ids.get(def.parent)
  else if (def.space === 'private') body.private = true
  else body.teamspace_id = spaceIds.get(def.space)
  const created = await api<{ id: string }>('POST', `${ws}/pages`, body)
  ids.set(def.title, created.id)
  existing.set(key, created.id)
  fresh.add(def.title)
  console.log(`+ page ${def.icon} ${def.title}`)
}

const ref = (title: string) => {
  const id = ids.get(title)
  if (!id) throw new Error(`unknown page "${title}"`)
  return id
}

function findImageUrls(blocks: unknown, out: string[] = []): string[] {
  if (!Array.isArray(blocks)) return out
  for (const block of blocks as Record<string, any>[]) {
    if (block?.type === 'image' && typeof block.props?.url === 'string' && block.props.url) out.push(block.props.url)
    findImageUrls(block?.children, out)
  }
  return out
}

let written = 0
for (const def of PAGES) {
  if (!def.body || !(fresh.has(def.title) || REFRESH)) continue
  const id = ref(def.title)
  const writer: Who = def.space === 'private' ? 'dev' : (def.editor ?? def.who ?? 'dev')
  as(writer)
  const page = await api<Page>('GET', `${ws}/pages/${id}`)
  const patch: Record<string, unknown> = { expected_version: page.version, icon: def.icon }

  if (def.cover && (!page.cover_url || REGEN_IMAGES)) {
    const file = await upload(`${ws}/pages/${id}/files`, `cover-${def.cover}.jpg`, await coverJpeg(def.cover))
    patch.cover_url = file.url
    patch.cover_position = def.coverPosition ?? '50,50'
  }

  const imageUrls = new Map<string, string>()
  const previous = findImageUrls(page.content)
  for (const [index, [key, render]] of Object.entries(def.images ?? {}).entries()) {
    const reuse = !REGEN_IMAGES ? previous[index] : undefined
    imageUrls.set(key, reuse ?? (await upload(`${ws}/pages/${id}/files`, `${key}.png`, await render())).url)
  }
  const ctx: Ctx = {
    ref,
    image: (key) => {
      const url = imageUrls.get(key)
      if (!url) throw new Error(`no image "${key}" on "${def.title}"`)
      return url
    },
  }
  patch.content = def.body(dsl(ctx))
  await api('PATCH', `${ws}/pages/${id}`, patch)
  written++
  console.log(`~ content ${def.title}${writer === 'member' ? ' (by member)' : ''}`)
}

as('dev')
const favorites = new Set((await api<{ items: { page_id: string }[] }>('GET', `${ws}/pages/favorites`)).items.map((item) => item.page_id))
for (const title of FAVORITES) {
  const id = ref(title)
  if (favorites.has(id)) continue
  await api('PUT', `${ws}/pages/${id}/favorite`)
  console.log(`★ favorite ${title}`)
}

console.log(`done: ${fresh.size} new page(s), ${written} page(s) written`)
