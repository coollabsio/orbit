// App Store screenshots of the mobile app's Expo web build (Metro web on 127.0.0.1:8082).
// Usage: node shoot.mjs <out dir> <server> <token file> <ids JSON from seed-demo.mjs> [iphone|ipad] [dark|light]
// iphone: 440x956 at 3x = 1320x2868 (6.9"). ipad: 1032x1376 at 2x = 2064x2752 (13").
import { chromium } from '/root/devel/orbit/node_modules/playwright/index.mjs'
import { readFileSync, mkdirSync } from 'node:fs'

const [OUT, SERVER, TOKEN_FILE, IDS, device = 'iphone', scheme = 'dark'] = process.argv.slice(2)
const NATIVE = 'http://127.0.0.1:8082'
const TOKEN = readFileSync(TOKEN_FILE, 'utf8').trim()
const ids = JSON.parse(IDS)
const DEVICES = {
  iphone: { viewport: { width: 440, height: 956 }, deviceScaleFactor: 3 },
  ipad: { viewport: { width: 1032, height: 1376 }, deviceScaleFactor: 2 },
}
const VIEWS = [
  ['01-home', '/'],
  ['02-tasks', '/tasks'],
  ['03-task', `/tasks/${ids.task}`],
  ['04-chat', `/chat/${ids.chat}`],
  ['05-docs', '/docs'],
  ['06-doc', `/docs/${ids.doc}`],
  ['07-activity', '/activity'],
]
mkdirSync(OUT, { recursive: true })
const browser = await chromium.launch()
const ctx = await browser.newContext({ ...DEVICES[device], isMobile: true, hasTouch: true, colorScheme: scheme })
await ctx.addInitScript(([server, token]) => localStorage.setItem('orbit.session', JSON.stringify({ server, token })), [SERVER, TOKEN])
const page = await ctx.newPage()
for (const [name, path] of VIEWS) {
  await page.goto(NATIVE + path, { waitUntil: 'networkidle' }).catch(() => {})
  await page.waitForTimeout(3000)
  await page.screenshot({ path: `${OUT}/${name}.png` })
  console.log(name)
}
await browser.close()
