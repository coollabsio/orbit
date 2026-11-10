// Renders 1920x1080 slide PNGs from a release spec and the screenshots in <spec dir>/build/shots.
// Usage: node marketing/release-video/slides.mjs <spec.json>
// Ported from Coolify's marketing/release-video (the v4.7 update video), in Orbit colors.
import { createRequire } from 'node:module'
import fs from 'node:fs'
import path from 'node:path'

const require = createRequire(new URL('../../package.json', import.meta.url))
const { chromium } = require('playwright')
const specPath = path.resolve(process.argv[2])
const spec = JSON.parse(fs.readFileSync(specPath, 'utf8'))
const build = path.join(path.dirname(specPath), 'build')
const shots = path.join(build, 'shots')
const out = path.join(build, 'slides')
fs.rmSync(out, { recursive: true, force: true })
fs.mkdirSync(out, { recursive: true })

// The app's own UI font: fallback fonts make letters collide.
const fontFile = require.resolve('@fontsource-variable/noto-sans/files/noto-sans-latin-wght-normal.woff2')
const font = fs.readFileSync(fontFile).toString('base64')
const v = spec.version
const pink = '#f2458f'

const css = `@font-face{font-family:Noto;src:url(data:font/woff2;base64,${font}) format('woff2');font-weight:100 900}
*{margin:0;box-sizing:border-box}
body{width:1920px;height:1080px;background:radial-gradient(1200px 700px at 80% -10%,#5c1636 0%,#0b0a0c 55%);font-family:Noto,sans-serif;color:#fff;overflow:hidden}
.k{display:inline-block;font-size:24px;font-weight:600;color:#ff9cc6;background:#f2458f26;border:1px solid #f2458f80;padding:6px 16px;border-radius:999px;letter-spacing:.5px}
h1{font-size:58px;font-weight:800;letter-spacing:-0.5px;margin-top:16px}
p.s{font-size:30px;color:#d2c9cd;margin-top:12px;line-height:1.35;max-width:1700px}
.wrap{position:absolute;left:50%;transform:translateX(-50%);top:222px;width:1600px;height:830px;border-radius:18px;overflow:hidden;border:1px solid #ffffff22;box-shadow:0 30px 80px #000c}
.wrap img{position:absolute;display:block;max-width:none}
.box{position:absolute;border:4px solid ${pink};border-radius:10px;box-shadow:0 0 0 9999px #00000070,0 0 30px ${pink}99}
.head{position:absolute;left:100px;top:56px;right:100px}
.brand{position:absolute;right:60px;top:40px;font-size:24px;color:#ffffff88;font-weight:700}
.c{display:flex;flex-direction:column;justify-content:center;height:100%;padding:0 160px}
ul{margin-top:40px;font-size:36px;line-height:1.75;color:#ececf0;list-style:none}
li:before{content:"›  ";color:${pink};font-weight:800}`

// `box` is [x, y, width, height] in the screenshot's CSS pixels (the PNG is 2x, so 1440x900 for a 2880x1800 PNG).
// The crop hides everything left of `hideLeft` (default 260: the app sidebar; Settings pages use about 224).
const W = 1600, H = 830
function pngSize(file) {
  const b = fs.readFileSync(file)
  return { w: b.readUInt32BE(16) / 2, h: b.readUInt32BE(20) / 2 }
}
function featureSlide(s) {
  const file = path.join(shots, `screenshot-${s.shot}.png`)
  const { w: SW, h: SH } = pngSize(file)
  const img = 'data:image/png;base64,' + fs.readFileSync(file).toString('base64')
  const f = W / SW
  const [x, y, w, h] = s.box
  const z = Math.max(1.17, Math.min(1.7, (W * 0.9) / (w * f), (H * 0.75) / (h * f)))
  const sc = f * z
  let ox = Math.max(0, Math.min(SW * sc - W, (x + w / 2) * sc - W / 2))
  ox = Math.min(Math.max(ox, (s.hideLeft ?? 260) * sc), SW * sc - W)
  const oy = Math.max(0, Math.min(SH * sc - H, (y + h / 2) * sc - H / 2 + (s.shiftDown ?? 0)))
  return `<div class="brand">Orbit ${v}</div>
<div class="head"><div class="k">${s.tag}</div><h1>${s.title}</h1></div>
<div class="wrap"><img src="${img}" style="width:${SW * sc}px;height:${SH * sc}px;left:${-ox}px;top:${-oy}px">
<div class="box" style="left:${x * sc - ox - 6}px;top:${y * sc - oy - 6}px;width:${w * sc + 12}px;height:${h * sc + 12}px"></div></div>`
}

function slideBody(s) {
  if (s.type === 'title') {
    return `<div class="c"><div class="k" style="width:fit-content">Product update</div>
<h1 style="font-size:120px;margin-top:30px">Orbit ${v}</h1><p class="s" style="font-size:44px;margin-top:24px">${s.subtitle}</p></div>`
  }
  if (s.type === 'list') {
    return `<div class="c"><div class="k" style="width:fit-content">${s.tag}</div><ul>${s.items.map((i) => `<li>${i}</li>`).join('')}</ul></div>`
  }
  if (s.type === 'outro') {
    return `<div class="c" style="align-items:center;text-align:center"><h1 style="font-size:96px">${s.title}</h1>
<p class="s" style="font-size:40px;margin-top:28px">${s.lines.join('<br>')}</p>
<p class="s" style="margin-top:60px;font-size:34px;color:#ffffff99">orbit.so</p></div>`
  }
  return featureSlide(s)
}

const browser = await chromium.launch()
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } })
for (const [i, s] of spec.slides.entries()) {
  await page.setContent(`<html><head><style>${css}</style></head><body>${slideBody(s)}</body></html>`)
  await page.evaluate(() => document.fonts.ready)
  await page.waitForTimeout(300)
  await page.screenshot({ path: path.join(out, String(i).padStart(2, '0') + '.png') })
}
await browser.close()
console.log(spec.slides.length, 'slides in', out)
