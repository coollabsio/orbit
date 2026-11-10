// Sign in to an Orbit instance and run screenshot scenes. Dark theme, 1440x900 at 2x (2880x1800 PNG).
// Usage: node marketing/release-video/shoot.mjs <out dir> [base URL] [scene names, comma separated]
// Example: node marketing/release-video/shoot.mjs marketing/releases/v0.4.0/build/shots http://127.0.0.1:19080 db-table
// Playwright comes from the repo root node_modules.
import { createRequire } from 'node:module'
import { mkdirSync } from 'node:fs'
import { scenes } from './scenes.mjs'

const require = createRequire(new URL('../../package.json', import.meta.url))
const { chromium } = require('playwright')
const [out, base = 'http://127.0.0.1:19080', only] = process.argv.slice(2)
if (!out) throw new Error('usage: node shoot.mjs <out dir> [base URL] [scenes]')
mkdirSync(out, { recursive: true })

const browser = await chromium.launch()
const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2, colorScheme: 'dark' })
const page = await context.newPage()
await page.goto(base + '/login')
await page.getByLabel('Email').fill('test@example.com')
await page.getByLabel('Password', { exact: true }).fill('password')
await page.getByRole('button', { name: 'Sign in' }).click()
await page.waitForURL((url) => !url.pathname.startsWith('/login'))
await page.waitForTimeout(1500)

const shot = async (name, { wait = 800, ...options } = {}) => {
  await page.mouse.move(5, 890) // no hover highlight in the picture
  await page.waitForTimeout(wait)
  await page.screenshot({ path: `${out}/screenshot-${name}.png`, ...options })
  console.log('shot', name)
}
const wanted = only ? only.split(',') : Object.keys(scenes)
try {
  for (const name of wanted) await scenes[name]({ page, shot, base, name })
} finally {
  await browser.close()
}
