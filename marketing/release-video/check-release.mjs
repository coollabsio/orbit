// Check that every image in a GitHub release's notes loads.
// Usage: node marketing/release-video/check-release.mjs v0.4.0
import { createRequire } from 'node:module'

const require = createRequire(new URL('../../package.json', import.meta.url))
const { chromium } = require('playwright')
const browser = await chromium.launch()
const page = await browser.newPage()
await page.goto(`https://github.com/coollabsio/orbit/releases/tag/${process.argv[2]}`, { waitUntil: 'networkidle' })
const images = await page.$$eval('.markdown-body img', (all) => all.map((i) => [i.alt, i.naturalWidth]))
await browser.close()
for (const [alt, width] of images) console.log(width > 0 ? 'ok    ' : 'BROKEN', alt)
if (!images.length || images.some(([, width]) => width === 0)) process.exit(1)
