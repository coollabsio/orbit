// Renders preview stills at the given frames: node scripts/stills.mjs 60 150 ...
import { bundle } from '@remotion/bundler'
import { renderStill, selectComposition } from '@remotion/renderer'
import path from 'node:path'

const frames = process.argv.slice(2).map(Number)
const serveUrl = await bundle({ entryPoint: path.resolve('src/index.ts') })
const composition = await selectComposition({ serveUrl, id: 'LaunchVideo', inputProps: {} })
for (const frame of frames) {
  await renderStill({ serveUrl, composition, frame, output: `out/stills/${String(frame).padStart(4, '0')}.jpg`, imageFormat: 'jpeg', jpegQuality: 85, scale: 0.5 })
  console.log('still', frame)
}
