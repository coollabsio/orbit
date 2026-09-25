// Captures marketing screenshots of the Orbit web app for the launch video.
// Usage (from apps/web so playwright-core resolves):
//   cd apps/web && bun ../../marketing/launch-video/scripts/capture.mjs [shotName ...]
// Mail / Chat / DM shots (mail mailReply chat chatThread dm) need the Vite from scripts/vite.capture.config.ts
// (enables the hidden routes + marketing seed in memory); docsImportFlow mocks the Notion import API itself.
// Env: ORBIT_URL (default http://127.0.0.1:18889), CHROME_PATH (auto-detected from ~/.cache/ms-playwright).
import { existsSync, readFileSync, readdirSync, writeFileSync, mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'

const here = dirname(fileURLToPath(import.meta.url))
const repo = resolve(here, '../../..')
const require = createRequire(join(repo, 'apps/web/package.json'))
const { chromium } = require('playwright-core')

const BASE = process.env.ORBIT_URL ?? 'http://127.0.0.1:18889'
const OUT = resolve(here, '../public/shots')
const MANIFEST = join(OUT, 'manifest.json')
mkdirSync(OUT, { recursive: true })

const DETAIL_TITLE = 'Realtime updates when a teammate edits a task'
const NEUTRAL = { x: 760, y: 24 } // empty stretch of the top bar

function findChrome() {
  if (process.env.CHROME_PATH) return process.env.CHROME_PATH
  const root = join(homedir(), '.cache/ms-playwright')
  const dirs = readdirSync(root).filter((d) => /^chromium-\d+$/.test(d)).sort().reverse()
  for (const d of dirs) {
    for (const sub of ['chrome-linux64', 'chrome-linux']) {
      const p = join(root, d, sub, 'chrome')
      if (existsSync(p)) return p
    }
  }
  throw new Error('No chromium found; set CHROME_PATH')
}

// Throwaway pages in the dev workspace that should not appear in marketing shots (filtered from API responses only;
// nothing is deleted). Override with HIDE_PAGE_TITLES="a|b|c" (empty string disables).
const HIDE_PAGE_TITLES = new Set((process.env.HIDE_PAGE_TITLES ?? 'asd|hallo|hello|Asdf|Ideas').split('|').filter(Boolean))

async function hideScratchPages(ctx) {
  if (!HIDE_PAGE_TITLES.size) return
  await ctx.route((url) => /\/api\/v1\/workspaces\/[^/]+\/pages(\/search)?$/.test(url.pathname), async (route) => {
    if (route.request().method() !== 'GET') return route.continue()
    const response = await route.fetch()
    const json = await response.json().catch(() => null)
    if (!json || !Array.isArray(json.items)) return route.fulfill({ response })
    json.items = json.items.filter((item) => !HIDE_PAGE_TITLES.has(item.title))
    await route.fulfill({ response, json })
  })
}

const NO_MOTION = '*,*::before,*::after{animation:none!important;transition:none!important;caret-color:transparent!important}'

const browser = await chromium.launch({ executablePath: findChrome() })

async function newPage(theme = 'dark', extraStorage = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 2 })
  await ctx.addInitScript(([theme, extra]) => {
    localStorage.setItem('theme', theme)
    localStorage.setItem('orbit:sidebar_collapsed', extra.collapsed ? 'true' : 'false')
  }, [theme, extraStorage])
  await hideScratchPages(ctx)
  const page = await ctx.newPage()
  await page.goto(`${BASE}/login`)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await page.waitForURL((u) => !u.pathname.startsWith('/login'))
  return page
}

async function settle(page) {
  await page.waitForLoadState('networkidle').catch(() => {})
  await page.addStyleTag({ content: NO_MOTION }).catch(() => {})
  await page.waitForTimeout(900)
  // nothing still loading?
  await page.waitForFunction(() => !document.querySelector('[aria-busy="true"], .animate-spin'), null, { timeout: 5000 }).catch(() => {})
}

async function go(page, path) {
  await page.goto(`${BASE}${path}`)
  await settle(page)
}

async function box(locator) {
  try {
    const l = locator.first()
    if (!(await l.count())) return null
    const b = await l.boundingBox()
    if (!b) return null
    const x = Math.max(0, b.x), y = Math.max(0, b.y)
    const w = Math.min(1600, b.x + b.width) - x, h = Math.min(1000, b.y + b.height) - y
    if (w <= 0 || h <= 0) return null
    return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) }
  } catch { return null }
}

function union(...boxes) {
  const bs = boxes.filter(Boolean)
  if (!bs.length) return null
  const x = Math.min(...bs.map((b) => b.x)), y = Math.min(...bs.map((b) => b.y))
  const r = Math.max(...bs.map((b) => b.x + b.w)), btm = Math.max(...bs.map((b) => b.y + b.h))
  return { x, y, w: r - x, h: btm - y }
}

const center = (b) => (b ? { x: Math.round(b.x + b.w / 2), y: Math.round(b.y + b.h / 2) } : null)

function pngSize(file) {
  const buf = readFileSync(file)
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) }
}

const manifest = existsSync(MANIFEST) ? JSON.parse(readFileSync(MANIFEST, 'utf8')) : []
function record(file, notes, regions, targets) {
  const clean = (o) => Object.fromEntries(Object.entries(o ?? {}).filter(([, v]) => v))
  const entry = { file, ...pngSize(join(OUT, file)), notes, regions: clean(regions), targets: clean(targets) }
  const i = manifest.findIndex((m) => m.file === file)
  if (i >= 0) manifest[i] = entry
  else manifest.push(entry)
  writeFileSync(MANIFEST, JSON.stringify(manifest, null, 2) + '\n')
  console.log('saved', file)
}

async function shoot(page, file, { mouse = NEUTRAL } = {}) {
  if (mouse) await page.mouse.move(mouse.x, mouse.y)
  await page.waitForTimeout(250)
  await page.screenshot({ path: join(OUT, file) })
}

// ---------- common region helpers ----------
const sidebar = (page) => box(page.locator('aside').first())
async function toolbarBox(page) {
  return union(
    await box(page.getByPlaceholder('Search tasks')),
    await box(page.getByRole('button', { name: 'Filter' })),
    await box(page.getByRole('button', { name: 'Sort' })),
    await box(page.getByRole('button', { name: 'Display' })),
    await box(page.getByRole('button', { name: 'New task' })),
  )
}

async function listRegions(page) {
  const groupHeader = page.locator('button[aria-label="Collapse Backlog"]').locator('xpath=..')
  const rowWithLabels = page.locator('[data-selected], a, div').filter({ hasText: 'Onboarding tour for first launch' }).last()
  return {
    regions: {
      sidebar: await sidebar(page),
      firstGroupHeader: await box(groupHeader),
      rowWithLabels: await box(page.getByRole('link', { name: /Onboarding tour for first launch/ }).or(rowWithLabels)),
      toolbar: await toolbarBox(page),
    },
    targets: {
      newTaskButton: center(await box(page.getByRole('button', { name: 'New task' }))),
      firstRowCheckbox: center(await box(page.getByRole('checkbox').first())),
      firstRowTitle: center(await box(page.getByText('Review the Orbit foundation'))),
      displayButton: center(await box(page.getByRole('button', { name: 'Display' }))),
      searchSidebar: center(await box(page.getByRole('button', { name: 'Search' }).first())),
    },
  }
}

// ---------- shots ----------
const shots = {
  async list(page) {
    await go(page, '/tasks?layout=list')
    const { regions, targets } = await listRegions(page)
    await shoot(page, 'list.png')
    record('list.png', 'All tasks, list layout grouped by status (dark).', regions, targets)
  },

  async board(page) {
    await go(page, '/tasks?layout=board')
    const cols = page.locator('.group\\/col')
    const card = page.locator('[data-board-card]')
    const regions = {
      sidebar: await sidebar(page),
      firstTwoColumns: union(await box(cols.nth(0)), await box(cols.nth(1))),
      boardArea: union(await box(cols.nth(0)), await box(cols.last())),
      card: await box(card.first()),
      toolbar: await toolbarBox(page),
    }
    const targets = {
      firstCard: center(await box(card.first())),
      secondColumnFirstCard: center(await box(cols.nth(1).locator('[data-board-card]').first())),
      newTaskButton: center(await box(page.getByRole('button', { name: 'New task' }))),
    }
    await shoot(page, 'board.png')
    record('board.png', 'Kanban board, all projects (dark).', regions, targets)
  },

  async roadmap(page) {
    await go(page, '/tasks?layout=timeline')
    const bars = page.locator('[aria-label*=", "][role="button"], [data-timeline-bar]')
    const regions = {
      sidebar: await sidebar(page),
      zoomControls: await box(page.getByRole('group', { name: 'Timeline zoom' })),
      todayMarker: await box(page.locator('[aria-label^="Today, "]')),
      toolbar: await toolbarBox(page),
    }
    // bars area = union of all visible bars
    const n = await bars.count()
    const bs = []
    for (let i = 0; i < n; i++) bs.push(await box(bars.nth(i)))
    regions.timelineBars = union(...bs)
    regions.firstBar = bs.find(Boolean) ?? null
    const targets = { firstBar: center(regions.firstBar), todayButton: center(await box(page.getByRole('button', { name: 'Today' }))) }
    await shoot(page, 'roadmap.png')
    record('roadmap.png', 'Roadmap / timeline layout with scheduled bars (dark).', regions, targets)
  },

  async myWeek(page) {
    await go(page, '/tasks?view=my_week&layout=list')
    const regions = {
      sidebar: await sidebar(page),
      rows: union(await box(page.locator('button[aria-label="Collapse Todo"]').locator('xpath=..')), await box(page.getByRole('checkbox', { name: /^Select / }).last())),
      myWeekNav: await box(page.getByRole('link', { name: 'My week' })),
    }
    if (regions.rows) regions.rows = { ...regions.rows, x: 224, w: 1376 }
    await shoot(page, 'my-week.png')
    record('my-week.png', 'My week view (dark).', regions, { myWeekNav: center(regions.myWeekNav) })
  },

  async detail(page) {
    const id = await taskIdByTitle(page, DETAIL_TITLE)
    await go(page, `/tasks/${id}`)
    const scroller = page.locator('div.overflow-y-auto').filter({ has: page.locator('aside') }).first()
    const regions = await detailRegions(page)
    await shoot(page, 'detail.png')
    record('detail.png', `Task detail: "${DETAIL_TITLE}" (description checklist, properties).`, regions.regions, regions.targets)
    const scrollable = await scroller.evaluate((el) => el.scrollHeight > el.clientHeight + 4).catch(() => false)
    if (scrollable) {
      await scroller.evaluate((el) => { el.scrollTop = el.scrollHeight })
      await page.waitForTimeout(400)
      const r2 = await detailRegions(page)
      await shoot(page, 'detail-bottom.png')
      record('detail-bottom.png', 'Task detail scrolled to relations, activity and comments.', r2.regions, r2.targets)
    }
  },

  async palette(page) {
    // the seeded wiki mentions "task" everywhere; keep this shot about tasks by showing only the top 2 page hits
    const cap = async (route) => {
      const response = await route.fetch()
      const json = await response.json()
      json.items = json.items.filter((item) => !HIDE_PAGE_TITLES.has(item.title)).slice(0, 2)
      await route.fulfill({ response, json })
    }
    await page.route('**/pages/search?*', cap)
    await go(page, '/tasks')
    await page.keyboard.press('Control+k')
    const input = page.getByPlaceholder('Search tasks, pages and navigation…')
    await input.waitFor()
    await input.fill(process.env.PALETTE_QUERY ?? 'task')
    await page.waitForLoadState('networkidle').catch(() => {})
    await page.waitForTimeout(1200)
    const dialog = page.getByRole('dialog')
    const regions = { dialog: await box(dialog), input: await box(input), results: await box(dialog.getByRole('listbox')) }
    const targets = { firstResult: center(await box(dialog.getByRole('option').first())) }
    await shoot(page, 'palette.png', { mouse: { x: 1590, y: 990 } })
    await page.unroute('**/pages/search?*', cap)
    record('palette.png', `Command palette (Ctrl+K) searching "${process.env.PALETTE_QUERY ?? 'task'}" (page hits capped at 2 so tasks show).`, regions, targets)
  },

  async bulk(page) {
    await go(page, '/tasks?layout=list')
    const boxes = page.getByRole('checkbox', { name: /^Select / })
    const picks = [1, 2, 4, 6]
    const picked = []
    for (const i of picks) {
      picked.push(await box(boxes.nth(i)))
      await boxes.nth(i).click({ force: true })
    }
    await settle(page)
    const bar = page.getByRole('toolbar', { name: 'Selected tasks' }).or(page.locator('[aria-label="Selected tasks"]'))
    const regions = { bulkBar: await box(bar), selectedRows: union(...picked.map((b) => b && { x: 224, y: b.y - 12, w: 1376, h: b.h + 24 })) }
    const targets = Object.fromEntries(picked.map((b, i) => [`checkbox${i + 1}`, center(b)]))
    await shoot(page, 'bulk.png', { mouse: { x: 760, y: 24 } })
    record('bulk.png', 'List with 4 rows selected and the floating bulk action bar.', regions, targets)
  },

  async workflow(page) {
    const pid = await projectIdByName(page, 'Orbit')
    await go(page, `/tasks/projects/${pid}/settings`)
    const regions = {
      statusesCard: await cardByHeading(page, 'Statuses'),
      generalCard: await cardByHeading(page, 'General'),
      addStatusButtons: union(await box(page.locator('button[aria-label^="Add "][aria-label$=" status"]').first()), await box(page.locator('button[aria-label^="Add "][aria-label$=" status"]').last())),
    }
    await shoot(page, 'workflow.png')
    record('workflow.png', 'Project settings for Orbit: workflow statuses by category.', regions, {})
  },

  async listLight() {
    const page = await newPage('light')
    await go(page, '/tasks?layout=list')
    const { regions, targets } = await listRegions(page)
    await shoot(page, 'list-light.png')
    record('list-light.png', 'All tasks, list layout (light theme).', regions, targets)
    await page.context().close()
  },

  async boardLight() {
    const page = await newPage('light')
    await go(page, '/tasks?layout=board')
    const cols = page.locator('.group\\/col')
    const regions = { firstTwoColumns: union(await box(cols.nth(0)), await box(cols.nth(1))), card: await box(page.locator('[data-board-card]').first()) }
    await shoot(page, 'board-light.png')
    record('board-light.png', 'Kanban board (light theme).', regions, { firstCard: center(regions.card) })
    await page.context().close()
  },

  async github(page) {
    await go(page, '/settings/github')
    // the local dev GitHub App has a throwaway name; show a neutral one in the shot (DOM only)
    await page.evaluate(() => {
      const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
      while (w.nextNode()) if (w.currentNode.nodeValue.includes('orbit-local-dev-delete-me')) w.currentNode.nodeValue = w.currentNode.nodeValue.replace('orbit-local-dev-delete-me', 'Orbit')
    })
    const regions = { githubCard: await cardByHeading(page, 'Installed repositories'), settingsNav: await box(page.getByRole('link', { name: 'General' }).locator('xpath=../..')), sidebar: await sidebar(page) }
    await shoot(page, 'github.png')
    record('github.png', 'Settings > GitHub: installed app and connected repositories (dev app name replaced with "Orbit" in the DOM).', regions, { installButton: center(await box(page.getByRole('button', { name: /Install or change/ }))) })
  },

  async apiTokens(page) {
    await go(page, '/settings/api-tokens')
    const regions = { createCard: await cardByHeading(page, 'Create API token'), activeCard: await cardByHeading(page, 'Active API tokens'), sidebar: await sidebar(page) }
    await shoot(page, 'api-tokens.png')
    record('api-tokens.png', 'Settings > API tokens: create form (scopes, projects, service account) and active tokens.', regions, { createButton: center(await box(page.getByRole('button', { name: 'Create token' }))), nameInput: center(await box(page.getByPlaceholder('Discord bot'))) })
  },

  async sidebarCollapsed() {
    const page = await newPage('dark', { collapsed: true })
    await go(page, '/tasks?layout=list')
    const { regions, targets } = await listRegions(page)
    await shoot(page, 'sidebar-collapsed.png')
    record('sidebar-collapsed.png', 'List layout with the sidebar collapsed to an icon rail.', regions, targets)
    await page.context().close()
  },

  async statusMenu(page) {
    const id = await taskIdByTitle(page, DETAIL_TITLE)
    await go(page, `/tasks/${id}`)
    const aside = page.locator('aside').last()
    const trigger = aside.getByRole('button').first()
    await trigger.click()
    const menu = page.getByRole('menu')
    await menu.waitFor()
    await page.waitForTimeout(500)
    const regions = { statusMenu: await box(menu), trigger: await box(trigger), properties: await box(aside) }
    await shoot(page, 'detail-status-menu.png', { mouse: null })
    record('detail-status-menu.png', 'Task detail with the status dropdown open (colored workflow statuses).', regions, { statusTrigger: center(regions.trigger) })
  },
}

// Bounding box of the bordered card that contains a heading with this exact text.
async function cardByHeading(page, text) {
  const handle = await page.evaluateHandle((text) => {
    const el = [...document.querySelectorAll('h1,h2,h3,h4,div,span,p')].find((n) => n.childElementCount === 0 && n.textContent.trim() === text)
    let cur = el
    while (cur && cur !== document.body) {
      const cls = typeof cur.className === 'string' ? cur.className : ''
      if (/\brounded/.test(cls) && /\bborder\b/.test(cls)) return cur
      cur = cur.parentElement
    }
    return null
  }, text)
  const el = handle.asElement()
  if (!el) return null
  const b = await el.boundingBox()
  return b && { x: Math.round(b.x), y: Math.round(b.y), w: Math.round(b.width), h: Math.round(Math.min(b.height, 1000 - b.y)) }
}

async function detailRegions(page) {
  const aside = page.locator('aside').last()
  const relations = page.getByRole('region', { name: 'Relations' }).or(page.locator('section[aria-label="Relations"]'))
  const activity = page.getByRole('heading', { name: 'Activity' }).locator('xpath=../..')
  const composer = page.getByPlaceholder('Leave a comment…')
  const labels = aside.locator('text=Labels').locator('xpath=..')
  const panel = page.locator('div.overflow-y-auto').filter({ has: aside }).first()
  return {
    regions: {
      detailPanel: await box(panel),
      title: await box(page.getByText(DETAIL_TITLE).first()),
      description: await box(page.getByText('Push task changes over the realtime channel').first()),
      checklist: await box(page.locator('ul[data-type="taskList"], input[type="checkbox"]').first().locator('xpath=ancestor::ul[1]')),
      properties: await box(aside),
      labels: await box(labels),
      relations: await box(relations),
      comments: union(await box(activity), await box(composer)),
      commentComposer: await box(composer),
    },
    targets: {
      closeButton: center(await box(page.getByRole('button', { name: 'Close task' }))),
      statusProperty: center(await box(aside.getByRole('button').first())),
      commentComposer: center(await box(composer)),
    },
  }
}

let cache
async function api(page) {
  if (cache) return cache
  cache = await page.evaluate(async () => {
    const ws = (await (await fetch('/api/v1/workspaces')).json()).find((w) => w.name === 'Orbit Development')
    const projects = (await (await fetch(`/api/v1/workspaces/${ws.id}/projects`)).json()).items
    const t = await (await fetch(`/api/v1/workspaces/${ws.id}/tasks?limit=200`)).json()
    return { ws, projects, tasks: t.items ?? t }
  })
  return cache
}
async function taskIdByTitle(page, title) { return (await api(page)).tasks.find((t) => t.title === title).id }
async function projectIdByName(page, name) { return (await api(page)).projects.find((p) => p.name === name).id }

// ---------- docs ----------
const treeSection = (page) => page.locator('section').filter({ has: page.locator('nav[aria-label="Pages"]') }).first()

async function docIds(page) {
  return page.evaluate(async () => {
    const ws = (await (await fetch('/api/v1/workspaces')).json()).find((w) => w.name === 'Orbit Development')
    const pages = (await (await fetch(`/api/v1/workspaces/${ws.id}/pages`)).json()).items
    return { ws: ws.id, ids: Object.fromEntries(pages.map((p) => [p.title, p.id])) }
  })
}
async function docId(page, title) {
  const { ids } = await docIds(page)
  if (!ids[title]) throw new Error(`no page "${title}"`)
  return ids[title]
}

// No "Saving…" in a shot: wait until the header save status is idle ("Edited …") or "Saved".
async function waitSaved(page) {
  await page.waitForFunction(() => {
    const el = document.querySelector('[data-save-status]')
    return el && !['pending', 'saving'].includes(el.getAttribute('data-save-status'))
  }, null, { timeout: 15000 }).catch(() => {})
}

async function openDoc(page, title, { expand = [] } = {}) {
  const id = await docId(page, title)
  await go(page, `/docs/${id}`)
  await page.locator('.bn-editor').first().waitFor({ timeout: 15000 })
  await expandTree(page, expand)
  // covers and images fully decoded
  await page.waitForFunction(() => [...document.images].every((img) => img.complete), null, { timeout: 10000 }).catch(() => {})
  await page.waitForTimeout(600)
  await waitSaved(page)
  return id
}

async function expandTree(page, titles) {
  if (!titles.length) return
  const { ids } = await docIds(page)
  for (const title of titles) {
    const row = page.locator(`[data-page-id="${ids[title]}"]:not([data-section="favorites"])`).first()
    const button = row.getByRole('button', { name: 'Expand', exact: true })
    if (await button.count()) await button.first().click()
  }
  // drop focus so the last clicked row does not keep its hover actions visible
  await page.evaluate(() => document.activeElement?.blur?.())
  await page.waitForTimeout(300)
}

async function blockBox(page, type, { all = false } = {}) {
  const blocks = page.locator(`.bn-editor [data-content-type="${type}"]`)
  const n = await blocks.count()
  const out = []
  for (let i = 0; i < n; i++) {
    const b = await box(blocks.nth(i))
    if (b && b.h > 4) out.push(b)
    if (b && !all) break
  }
  return all ? union(...out) : out[0] ?? null
}

// Scrolls the document pane so `selector` sits `offset` px below the top of the viewport.
async function scrollDocTo(page, selector, offset = 90) {
  await page.evaluate(([selector, offset]) => {
    const el = document.querySelector(selector)
    if (!el) return
    let scroller = el.parentElement
    while (scroller && !(scroller.scrollHeight > scroller.clientHeight + 2 && /(auto|scroll)/.test(getComputedStyle(scroller).overflowY))) scroller = scroller.parentElement
    if (!scroller) return
    scroller.scrollTop += el.getBoundingClientRect().top - offset
  }, [selector, offset])
  await page.waitForTimeout(400)
}

async function docRegions(page) {
  const favorites = page.getByRole('group', { name: 'Favorites' })
  const cover = page.locator('.group\\/cover').first()
  return {
    tree: await box(treeSection(page)),
    favorites: await box(favorites),
    cover: await box(cover),
    icon: await box(page.getByRole('button', { name: 'Change icon' })),
    title: await box(page.getByLabel('Page title')),
    content: await box(page.locator('.bn-editor').first()),
    breadcrumbs: await box(page.locator('nav[aria-label="Page path"]')),
    star: await box(page.getByRole('button', { name: 'Remove from favorites' })),
    saveStatus: await box(page.locator('[data-save-status]')),
  }
}

async function docTargets(page) {
  return {
    star: center(await box(page.getByRole('button', { name: /favorites$/ }))),
    newPage: center(await box(page.getByRole('button', { name: 'New page' }))),
    docsNav: center(await box(page.getByRole('link', { name: 'Docs' }).first())),
  }
}

const docShots = {
  async paletteDocs(page) {
    const query = process.env.PALETTE_DOCS_QUERY ?? 'roadmap'
    await go(page, '/tasks')
    await page.keyboard.press('Control+k')
    const input = page.getByPlaceholder('Search tasks, pages and navigation…')
    await input.waitFor()
    await input.fill(query)
    await page.waitForLoadState('networkidle').catch(() => {})
    await page.waitForTimeout(1500)
    const dialog = page.getByRole('dialog')
    const pageOption = dialog.getByRole('option').filter({ hasText: /Page ·/ }).first()
    const regions = { dialog: await box(dialog), input: await box(input), results: await box(dialog.getByRole('listbox')), pageResult: await box(pageOption) }
    const targets = { firstResult: center(await box(dialog.getByRole('option').first())), pageResult: center(regions.pageResult) }
    await shoot(page, 'palette-docs.png', { mouse: { x: 1590, y: 990 } })
    record('palette-docs.png', `Command palette searching "${query}": matching pages and tasks together.`, regions, targets)
  },

  async docsPage(page, file = 'docs-page.png', note = 'Docs page "Welcome to Orbit": cover, icon, title, rich content; full doc tree (dark).') {
    await openDoc(page, 'Welcome to Orbit', { expand: ['Welcome to Orbit'] })
    const regions = await docRegions(page)
    regions.checklist = await blockBox(page, 'checkListItem', { all: true })
    regions.table = await blockBox(page, 'table')
    await shoot(page, file)
    record(file, note, regions, await docTargets(page))
  },

  async docsRich(page) {
    await openDoc(page, 'Architecture overview', { expand: ['RFCs'] })
    await scrollDocTo(page, '.bn-editor [data-content-type="image"]', 54)
    const regions = {
      tree: await box(treeSection(page)),
      codeBlock: await blockBox(page, 'codeBlock'),
      table: await blockBox(page, 'table'),
      image: await blockBox(page, 'image'),
      checklist: await blockBox(page, 'checkListItem', { all: true }),
      numberedList: await blockBox(page, 'numberedListItem', { all: true }),
      callout: await box(page.locator('.bn-editor [data-content-type="paragraph"][data-background-color]').first()),
      content: await box(page.locator('.bn-editor').first()),
    }
    await shoot(page, 'docs-rich.png')
    record('docs-rich.png', 'Docs page "Architecture overview" scrolled to the table, code block and checklist (dark).', regions, {})
  },

  async docsSlash(page) {
    const title = process.env.SLASH_PAGE ?? 'Design principles'
    const id = await openDoc(page, title)
    const { ws } = await docIds(page)
    const original = await page.evaluate(async ([ws, id]) => (await (await fetch(`/api/v1/workspaces/${ws}/pages/${id}`)).json()).content, [ws, id])
    try {
      // new empty line right after the (one-line) intro paragraph, so the menu opens over real content
      const intro = page.locator('.bn-editor [data-content-type="paragraph"]').first()
      await intro.click()
      await page.keyboard.press('End')
      await page.keyboard.press('Enter')
      await page.keyboard.type('/')
      const menu = page.locator('.bn-suggestion-menu, [role="listbox"]').first()
      await menu.waitFor({ timeout: 5000 })
      await page.waitForTimeout(700)
      const caret = await page.evaluate(() => {
        const sel = window.getSelection()
        const block = sel?.anchorNode?.parentElement?.closest('.bn-block-content')
        const r = (block ?? sel.getRangeAt(0)).getBoundingClientRect()
        return { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height) }
      })
      const regions = { slashMenu: await box(menu), caretLine: caret, content: await box(page.locator('.bn-editor').first()), tree: await box(treeSection(page)) }
      // hide the transient save status text so the shot shows no "Saving…"
      await page.addStyleTag({ content: '[data-save-status]{visibility:hidden!important}' })
      await shoot(page, 'docs-slash.png', { mouse: null })
      record('docs-slash.png', `Slash menu open on an empty line of "${title}" (block options).`, regions, { caret: center(caret), slashMenu: center(regions.slashMenu) })
    } finally {
      await page.keyboard.press('Escape').catch(() => {})
      await page.goto(`${BASE}/tasks`).catch(() => {})
      await page.waitForTimeout(1500)
      // put the page content back exactly as the seed left it
      await page.evaluate(async ([ws, id, content]) => {
        const url = `/api/v1/workspaces/${ws}/pages/${id}`
        const current = await (await fetch(url)).json()
        await fetch(url, { method: 'PATCH', headers: { 'content-type': 'application/json', 'x-orbit-contract': 'orbit-api-v1' }, body: JSON.stringify({ expected_version: current.version, content }) })
      }, [ws, id, original])
    }
  },

  async docsTree(page) {
    await openDoc(page, 'RFC 012: Realtime sync', { expand: ['Meeting notes'] })
    // collapse Favorites for this shot so every section (down to Private) fits; restored below (it persists)
    const favHeader = page.locator('[role="group"][aria-label="Favorites"] [role="treeitem"][aria-expanded]').first()
    const collapseFavorites = (await favHeader.getAttribute('aria-expanded').catch(() => null)) === 'true'
    if (collapseFavorites) await favHeader.click()
    await page.evaluate(() => document.activeElement?.blur?.())
    await page.waitForTimeout(300)
    const group = (name) => box(page.getByRole('group', { name, exact: true }))
    const regions = {
      tree: await box(treeSection(page)),
      favorites: await group('Favorites'),
      engineering: await group('Engineering'),
      product: await group('Product'),
      company: await group('Company'),
      defaultSpace: await box(page.locator('nav[aria-label="Pages"] [role="group"]').nth(1)),
      private: await group('Private'),
      rfcChildren: union(
        await box(page.locator('nav[aria-label="Pages"] [role="treeitem"]:not([data-section="favorites"])').filter({ hasText: 'RFC 012' }).first()),
        await box(page.locator('nav[aria-label="Pages"] [role="treeitem"]:not([data-section="favorites"])').filter({ hasText: 'RFC 014' }).first()),
      ),
      activeRow: await box(page.locator('nav[aria-label="Pages"] [data-active]:not([data-section="favorites"])').first()),
    }
    await shoot(page, 'docs-tree.png')
    if (collapseFavorites) await favHeader.click()
    record('docs-tree.png', 'Doc tree: Favorites, teamspaces (Engineering / Product / Company) with nested pages, Private.', regions, await docTargets(page))
  },

  async docsRoadmap(page) {
    await openDoc(page, 'Q4 2026 roadmap')
    await scrollDocTo(page, '.bn-editor [data-content-type="checkListItem"]', 170)
    const regions = {
      tree: await box(treeSection(page)),
      checklist: await blockBox(page, 'checkListItem', { all: true }),
      pageLink: await blockBox(page, 'page'),
      pageLinks: await blockBox(page, 'page', { all: true }),
      callout: await box(page.locator('.bn-editor [data-content-type="paragraph"][data-background-color]').first()),
      content: await box(page.locator('.bn-editor').first()),
    }
    await shoot(page, 'docs-roadmap.png')
    record('docs-roadmap.png', 'Docs page "Q4 2026 roadmap": theme headings, checklist and page link blocks to RFCs (dark).', regions, { pageLink: center(regions.pageLink) })
  },

  async docsImport(page) {
    await go(page, '/docs/import')
    await page.getByText('Connect Notion').first().waitFor({ timeout: 10000 })
    await page.waitForTimeout(500)
    const pane = page.locator('section').filter({ has: page.getByRole('heading', { name: 'Import from Notion' }) }).last()
    const regions = {
      importPane: await box(pane),
      connectCard: await cardByHeading(page, 'Connect Notion'),
      tokenInput: await box(page.locator('input[type="password"]')),
      recentImports: await box(page.getByRole('region', { name: 'Recent imports' }).or(page.locator('section[aria-label="Recent imports"]'))),
      tree: await box(treeSection(page)),
    }
    await shoot(page, 'docs-import.png')
    record('docs-import.png', 'Docs > Import from Notion: connect step (token field, no token submitted).', regions, { tokenInput: center(regions.tokenInput) })
  },

  async docsLight() {
    const page = await newPage('light')
    await docShots.docsPage(page, 'docs-light.png', 'Docs page "Welcome to Orbit" (light theme).')
    await page.context().close()
  },

  async docsFavorite(page) {
    await openDoc(page, 'Q4 2026 roadmap')
    const regions = await docRegions(page)
    regions.header = union(regions.breadcrumbs, regions.star, await box(page.getByRole('button', { name: 'Page options' }).last()))
    await shoot(page, 'docs-favorite.png')
    record('docs-favorite.png', 'Docs page "Q4 2026 roadmap" header: breadcrumbs with teamspace, save status, filled favorite star; page listed under Favorites.', regions, await docTargets(page))
  },
}
Object.assign(shots, docShots)

// ---------- mail / chat (upcoming; needs the Vite from vite.capture.config.ts, which enables the routes) ----------
// Box of the nearest ancestor of `selector` (inclusive) whose class matches `classRe`, or whose parent matches `parentSel`.
async function ancestorBox(page, selector, { classRe = null, parentSel = null } = {}) {
  const r = await page.evaluate(([selector, classSrc, parentSel]) => {
    let el = document.querySelector(selector)
    const re = classSrc ? new RegExp(classSrc) : null
    while (el && el !== document.body) {
      if (re && typeof el.className === 'string' && re.test(el.className)) break
      if (parentSel && el.parentElement?.matches(parentSel)) break
      el = el.parentElement
    }
    if (!el || el === document.body) return null
    const b = el.getBoundingClientRect()
    return { x: b.x, y: b.y, width: b.width, height: b.height }
  }, [selector, classRe?.source ?? null, parentSel])
  if (!r) return null
  const x = Math.max(0, r.x), y = Math.max(0, r.y)
  const w = Math.min(1600, r.x + r.width) - x, h = Math.min(1000, r.y + r.height) - y
  return w > 0 && h > 0 ? { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) } : null
}

async function assertRoute(page, prefix) {
  if (!new URL(page.url()).pathname.startsWith(prefix)) {
    throw new Error(`redirected to ${page.url()}: is the capture Vite (scripts/vite.capture.config.ts) running on ${BASE}?`)
  }
}

async function openMailThread(page) {
  await go(page, '/mail/m_1?folder=f_inbox')
  await assertRoute(page, '/mail/')
  await page.locator('[data-slot="mail-message"]').first().waitFor()
  await page.waitForTimeout(300)
}

async function mailRegions(page) {
  return {
    folders: await box(page.locator('.group\\/mail > nav').first()),
    threadList: await box(page.locator('.group\\/mail > section').nth(0)),
    reader: await box(page.locator('.group\\/mail > section').nth(1)),
    selectedThread: await box(page.locator('.group\\/mail > section').nth(0).locator('.group\\/row[data-active="true"]')),
    readerToolbar: await box(page.locator('.group\\/mail > section').nth(1).locator('> div').first()),
    sidebar: await sidebar(page),
  }
}

const chatShots = {
  async mail(page) {
    await openMailThread(page)
    const regions = await mailRegions(page)
    regions.expandedMessage = await box(page.locator('[data-slot="mail-message"][data-expanded="true"]'))
    await shoot(page, 'mail.png')
    record('mail.png', 'Mail (upcoming, mock data): folder rail, inbox thread list, reader open on a 4-message customer thread (last one expanded).', regions, {
      replyButton: center(await box(page.locator('[data-slot="mail-message"][data-expanded="true"]').getByRole('button', { name: 'Reply' }))),
      selectedThread: center(regions.selectedThread),
      mailNav: center(await box(page.getByRole('link', { name: 'Mail' }).first())),
    })
  },

  async mailReply(page) {
    await openMailThread(page)
    await page.locator('[data-slot="mail-message"][data-expanded="true"]').getByRole('button', { name: 'Reply' }).click()
    const textarea = page.getByPlaceholder('Write a reply…')
    await textarea.waitFor()
    await textarea.evaluate((el) => { el.spellcheck = false }) // no red squiggles in the shot
    await textarea.pressSequentially(process.env.MAIL_REPLY ?? 'Thursday at 15:00 CET works great! I will send an invite, and we can run the Notion import live on a copy of your wiki.\n\nSee you then,\nMaya', { delay: 2 })
    // bring the whole composer (incl. Send) into view
    await page.evaluate(() => { const el = document.querySelector('.group\\/mail > section:nth-of-type(2) .overflow-y-auto'); if (el) el.scrollTop = el.scrollHeight })
    await page.waitForTimeout(900)
    const composer = await ancestorBox(page, 'textarea[placeholder="Write a reply…"]', { classRe: /\bmt-6\b/ })
    const regions = { composer, reader: await box(page.locator('.group\\/mail > section').nth(1)), threadList: await box(page.locator('.group\\/mail > section').nth(0)) }
    await shoot(page, 'mail-reply.png', { mouse: { x: 1590, y: 990 } })
    record('mail-reply.png', 'Mail thread with the inline reply composer open and a short reply typed (not sent).', regions, {
      sendButton: center(await box(page.getByRole('button', { name: 'Send', exact: true }))),
      composer: center(composer),
    })
  },

  async chat(page) {
    await go(page, '/chat/c_engineering')
    await assertRoute(page, '/chat/')
    await page.locator('#message-cm_reactions').waitFor()
    await page.waitForTimeout(400)
    const regions = {
      channelSidebar: await box(page.locator('.group\\/chat > div').first()),
      messages: await ancestorBox(page, '#message-cm_reactions', { classRe: /\boverflow-y-auto\b/ }),
      composer: await ancestorBox(page, 'textarea[placeholder="Message #engineering"]', { classRe: /\bpx-3 pb-2\b/ }),
      reactionMessage: await box(page.locator('#message-cm_reactions')),
      codeBlock: await box(page.locator('#message-cm_code pre')),
      githubChip: await box(page.locator('[id^="message-"] a[href*="github.com/orbit-hq"]')),
      mention: await box(page.locator('[data-mention="true"]')),
    }
    await shoot(page, 'chat.png')
    record('chat.png', 'Chat (upcoming, mock data): #engineering with grouped messages, a Rust code block, GitHub PR chip + GitHub bot post, reactions, @mention highlight, thread preview card, member list.', regions, {
      reactionMessage: center(regions.reactionMessage),
      threadPreview: center(await box(page.locator('#message-cm_thread_root').getByRole('button').filter({ hasText: /Messages/ }))),
      composer: center(regions.composer),
    })
  },

  async chatThread(page) {
    await go(page, '/chat/c_engineering?thread=cm_thread_root')
    await assertRoute(page, '/chat/')
    await page.getByRole('button', { name: 'Close thread' }).waitFor()
    await page.waitForTimeout(400)
    await page.evaluate(() => document.activeElement?.blur?.()) // thread composer autofocuses (focus ring)
    const regions = {
      threadPanel: await ancestorBox(page, 'button[aria-label="Close thread"]', { parentSel: '.group\\/chat' }),
      messages: await ancestorBox(page, '#message-cm_reactions', { classRe: /\boverflow-y-auto\b/ }),
      channelSidebar: await box(page.locator('.group\\/chat > div').first()),
      threadRoot: await box(page.locator('#message-cm_thread_root').first()),
    }
    await shoot(page, 'chat-thread.png')
    record('chat-thread.png', 'Chat thread "Rebase stale text patches?" open in the side panel next to #engineering (6 replies, code snippet, reactions).', regions, {
      closeThread: center(await box(page.getByRole('button', { name: 'Close thread' }))),
    })
  },

  async dm(page) {
    await go(page, '/dm/dm_andras')
    await assertRoute(page, '/dm/')
    await page.locator('[id^="message-"]').first().waitFor()
    await page.waitForTimeout(400)
    const regions = {
      dmList: await box(page.locator('.group\\/chat > aside').first()),
      conversation: await box(page.locator('.group\\/chat > div').first()),
      composer: await ancestorBox(page, '.group\\/chat textarea', { classRe: /\bpx-3 pb-2\b/ }),
    }
    await shoot(page, 'dm.png')
    record('dm.png', 'Direct messages (upcoming, mock data): conversation with Leo about the 0.9 release (code block, GitHub issue chip, reactions).', regions, {})
  },
}
Object.assign(shots, chatShots)

// ---------- Notion import flow (API mocked with page.route; no token is ever sent anywhere) ----------
const IMPORT_ID = '0192f1a4-7c3e-7d21-9b8e-5e1f4a6c2d90'
const NOTION_TREE = (() => {
  const nodes = []
  let n = 0
  const add = (title, icon, parent = null, kind = 'page') => {
    n += 1
    const id = `2a1c${String(n).padStart(4, '0')}-8f3e-4b1d-9c7a-1e5d3f${String(n).padStart(6, '0')}`
    nodes.push({ notion_id: id, parent_id: parent, title, icon, kind, child_count: 0 })
    if (parent) nodes.find((x) => x.notion_id === parent).child_count += 1
    return id
  }
  const eng = add('Engineering Wiki', '🛠️')
  const arch = add('Architecture', '🏗️', eng)
  add('Service map', null, arch)
  add('Data model', null, arch)
  add('RFC: Realtime sync', '⚡', arch)
  add('Onboarding for engineers', '👋', eng)
  const runbooks = add('Runbooks', '📕', eng)
  add('Deploying to production', null, runbooks)
  add('Incident response', '🚨', runbooks)
  add('Database backups', null, runbooks)
  add('Coding standards', null, eng)
  const product = add('Product Specs', '🎯')
  add('Notion import', null, product)
  add('My week view', null, product)
  add('Mobile app v1', '📱', product)
  add('Feature requests', null, product, 'database')
  const meetings = add('Meeting Notes', '🗓️', null, 'database')
  add('Weekly sync, Sep 21', null, meetings)
  add('Design review, Sep 18', null, meetings)
  add('Q4 planning kickoff', null, meetings)
  const handbook = add('Company Handbook', '📘')
  add('Values', null, handbook)
  add('Benefits & time off', null, handbook)
  add('Remote work policy', null, handbook)
  const archive = add('Archive 2024', '🗄️')
  add('Old roadmap', null, archive)
  add('Hackathon ideas', null, archive)
  return nodes
})()
const IMPORTED_ROOTS = ['Engineering Wiki', 'Product Specs', 'Meeting Notes', 'Company Handbook']

function mockImport(ws, phase, { count = 20, destination = null } = {}) {
  const now = new Date().toISOString()
  const base = {
    id: IMPORT_ID, workspace_id: ws, created_at: new Date(Date.now() - 3 * 60_000).toISOString(), updated_at: now,
    notion_workspace_name: 'Orbit HQ', error: null, destination: null, report: null, root_page_ids: [], tree: null,
    progress: { done: 0, failed: 0, total: 0 },
  }
  if (phase === 'ready') return { ...base, status: 'ready', tree: { nodes: NOTION_TREE, truncated: false, incomplete: false } }
  const done = Math.round(count * 0.65)
  if (phase === 'importing') return { ...base, status: 'importing', destination, progress: { done, failed: 0, total: count } }
  return {
    ...base, status: 'completed', destination, progress: { done: count, failed: 0, total: count },
    root_page_ids: IMPORTED_ROOTS.map((_, i) => importedPageId(i)),
    report: { failures: [], files_imported: 38, lossy: {}, missing_files: 0, previously_imported: 0, skipped: {}, truncated_blocks: 0, unfilled_pages_trashed: 0, unresolved_page_links: 0 },
  }
}
const importedPageId = (i) => `0192f1a5-0000-7000-8000-00000000000${i}`
const NODE_ICONS = Object.fromEntries(NOTION_TREE.map((n) => [n.title, n.icon]))

const importShots = {
  async docsImportFlow(page) {
    const { ws } = await docIds(page)
    const state = { phase: 'ready', count: 20, destination: null }
    const importUrl = (url) => /\/api\/v1\/workspaces\/[^/]+\/imports\/notion(\/|$)/.test(url.pathname)
    const handler = async (route) => {
      const req = route.request()
      const path = new URL(req.url()).pathname
      if (req.method() === 'POST' && path.endsWith('/start')) {
        const body = req.postDataJSON() ?? {}
        state.destination = { parent_page_id: body.destination?.parent_page_id ?? null, private: Boolean(body.destination?.private), teamspace_id: body.destination?.teamspace_id ?? null }
        state.phase = 'importing'
        return route.fulfill({ json: mockImport(ws, state.phase, state) })
      }
      if (req.method() !== 'GET') return route.fulfill({ status: 409, json: { code: 'notion_import_state_conflict', title: 'mocked' } })
      if (path.endsWith('/imports/notion')) return route.fulfill({ json: { items: [mockImport(ws, state.phase, state)] } })
      return route.fulfill({ json: mockImport(ws, state.phase, state) })
    }
    // once complete, the imported root pages exist in the page tree (injected into the list response only)
    const pagesHandler = async (route) => {
      if (route.request().method() !== 'GET') return route.continue()
      const response = await route.fetch()
      const json = await response.json().catch(() => null)
      if (!json || !Array.isArray(json.items)) return route.fulfill({ response })
      json.items = json.items.filter((item) => !HIDE_PAGE_TITLES.has(item.title))
      if (state.phase === 'completed') {
        const teamspace = state.destination?.teamspace_id ?? json.items.find((p) => p.teamspace_id)?.teamspace_id ?? null
        IMPORTED_ROOTS.forEach((title, i) => json.items.push({
          id: importedPageId(i), title, icon: NODE_ICONS[title] ?? null, parent_id: null, position: 900000 + i,
          private: false, teamspace_id: teamspace, updated_at: new Date().toISOString(), version: 1,
        }))
      }
      await route.fulfill({ response, json })
    }
    const pagesUrl = (url) => /\/api\/v1\/workspaces\/[^/]+\/pages$/.test(url.pathname)
    await page.route(importUrl, handler)
    await page.route(pagesUrl, pagesHandler)
    try {
      // ---- choose ----
      await go(page, `/docs/import/${IMPORT_ID}`)
      await page.getByRole('tree', { name: 'Notion pages' }).waitFor({ timeout: 10000 })
      for (const title of ['Engineering Wiki', 'Architecture', 'Product Specs', 'Meeting Notes']) {
        await page.getByRole('button', { name: `Expand ${title}`, exact: true }).click()
      }
      for (const title of ['Mobile app v1', 'Q4 planning kickoff', 'Archive 2024']) {
        await page.getByRole('checkbox', { name: `Select ${title}`, exact: true }).click()
      }
      // destination: a named teamspace (Engineering if it exists)
      const want = process.env.IMPORT_TEAMSPACE ?? 'Engineering'
      await page.getByRole('combobox', { name: 'Import into' }).click()
      const option = page.locator('[role="option"]').filter({ hasText: new RegExp(`^\\s*${want}\\s*$`) })
      await page.locator('[role="option"]').first().waitFor({ timeout: 3000 }).catch(() => {})
      if (await option.count()) await option.first().click()
      await page.waitForTimeout(400)
      if (await page.locator('[role="listbox"]').count()) await page.keyboard.press('Escape')
      await page.waitForTimeout(400)
      await page.evaluate(() => document.activeElement?.blur?.())
      const countText = await page.getByTestId('selected-count').innerText()
      state.count = Number.parseInt(countText, 10) || state.count
      const importButton = page.getByRole('button', { name: /^Import \d+ pages?$/ })
      const destination = union(
        await ancestorBox(page, '#notion-import-space', { classRe: /\bgrid\b/ }),
        await ancestorBox(page, '#notion-import-parent', { classRe: /\bgrid\b/ }),
      )
      const chooseRegions = {
        chooser: await box(page.getByRole('tree', { name: 'Notion pages' })),
        selectionCount: await box(page.getByTestId('selected-count')),
        destination,
        importButton: await box(importButton),
        header: await ancestorBox(page, '[data-testid="selected-count"]', { classRe: /\bborder-b\b/ }),
        tree: await box(treeSection(page)),
      }
      await shoot(page, 'docs-import-choose.png')
      record('docs-import-choose.png', `Notion import, "Choose pages" step (mocked API): Notion workspace tree with tri-state checkboxes, ${state.count} pages selected, destination teamspace.`, chooseRegions, {
        importButton: center(chooseRegions.importButton),
        firstCheckbox: center(await box(page.getByRole('checkbox', { name: 'Select Engineering Wiki', exact: true }))),
      })

      // ---- importing (the real start mutation hits the mocked /start) ----
      await importButton.click()
      await page.getByRole('progressbar', { name: 'Import progress' }).waitFor({ timeout: 10000 })
      await page.waitForTimeout(700)
      const progressCard = await box(page.locator('section[role="status"]'))
      await shoot(page, 'docs-import-progress.png')
      record('docs-import-progress.png', `Notion import, "Importing" step (mocked): progress bar at ~65% (${Math.round(state.count * 0.65)} of ${state.count} pages).`, { progressCard, progressBar: await box(page.getByRole('progressbar')) }, {})

      // ---- completed (next poll picks it up; pages are refetched with the imported roots) ----
      state.phase = 'completed'
      await page.getByRole('heading', { name: 'Import complete' }).waitFor({ timeout: 15000 })
      await settle(page)
      await page.getByRole('region', { name: 'Imported pages' }).getByText('Engineering Wiki').waitFor({ timeout: 10000 }).catch(() => {})
      await page.waitForTimeout(500)
      const resultCard = await box(page.getByRole('heading', { name: 'Import complete' }).locator('xpath=ancestor::div[contains(@class,"max-w-2xl")][1]'))
      await shoot(page, 'docs-import-done.png')
      record('docs-import-done.png', 'Notion import, "Complete" step (mocked): stats (pages / files / failed), links to the imported root pages, which also appear in the doc tree.', {
        resultCard,
        stats: await box(page.getByText('Pages imported', { exact: true }).locator('xpath=ancestor::div[contains(@class,"grid-cols-3")][1]')),
        importedPages: await box(page.getByRole('region', { name: 'Imported pages' })),
        tree: await box(treeSection(page)),
      }, { openImported: center(await box(page.getByRole('button', { name: 'Open imported pages' }))) })
    } finally {
      await page.unroute(importUrl, handler)
      await page.unroute(pagesUrl, pagesHandler)
    }
  },
}
Object.assign(shots, importShots)

const wanted = process.argv.slice(2)
const main = await newPage('dark')
for (const [name, fn] of Object.entries(shots)) {
  if (wanted.length && !wanted.includes(name)) continue
  try { await fn(main) } catch (err) { console.error(`FAILED ${name}:`, err.message) }
}
await browser.close()
console.log('manifest:', MANIFEST)
