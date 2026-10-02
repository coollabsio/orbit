import { defineConfig } from 'vite'
import react, { reactCompilerPreset } from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import babel from '@rolldown/plugin-babel'
import openapi from './src/api/generated/openapi.json' with { type: 'json' }
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, readFileSync } from 'node:fs'
import { basename } from 'node:path'
import { fileURLToPath } from 'node:url'

const buildRevision = process.env.ORBIT_BUILD_REVISION ?? 'development'
const sourceHtml = readFileSync(new URL('./index.html', import.meta.url), 'utf8')
const bootstrapScript = sourceHtml.match(/<script>([\s\S]*?)<\/script>/)?.[1]
if (!bootstrapScript) throw new Error('theme bootstrap script is missing')
const cspScriptHash = `sha256-${createHash('sha256').update(bootstrapScript).digest('base64')}`

/**
 * Emoji files that Orbit serves itself (the production CSP has no CDN): the Twemoji images at `/assets/twemoji-<version>/<code>.svg`
 * and the emoji data at `/assets/emojibase-<version>/en/…`. The version in the path makes them immutable: the server
 * caches `/assets` for a year, so a browser downloads each file once. They stay in `node_modules`: the dev server
 * reads them from there and the build copies them into `dist`. The versions are those of the installed packages, as in
 * `src/lib/twemoji.ts`. The images are the `jdecked/twemoji` repository: its npm package of images stops at 15.0.0.
 */
const packageVersion = (name: string): string =>
  JSON.parse(readFileSync(new URL(`./node_modules/${name}/package.json`, import.meta.url), 'utf8')).version
const twemojiVersion = packageVersion('twemoji-assets')
const emojibaseVersion = packageVersion('emojibase-data')
const emojiAssets = [
  { url: `/assets/twemoji-${twemojiVersion}/`, dir: fileURLToPath(new URL('./node_modules/twemoji-assets/assets/svg/', import.meta.url)), files: /^[0-9a-f-]+\.svg$/, type: 'image/svg+xml' },
  { url: `/assets/emojibase-${emojibaseVersion}/en/shortcodes/`, dir: fileURLToPath(new URL('./node_modules/emojibase-data/en/shortcodes/', import.meta.url)), files: /^github\.json$/, type: 'application/json' },
  { url: `/assets/emojibase-${emojibaseVersion}/en/`, dir: fileURLToPath(new URL('./node_modules/emojibase-data/en/', import.meta.url)), files: /^(data|messages)\.json$/, type: 'application/json' },
]

// https://vite.dev/config/
export default defineConfig({
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  plugins: [
    react(),
    tailwindcss(),
    babel({ presets: [reactCompilerPreset()] }),
    {
      name: 'orbit-emoji-assets',
      configureServer(server) {
        server.middlewares.use((request, response, next) => {
          const path = (request.url ?? '').split('?')[0]
          const asset = emojiAssets.find((candidate) => path.startsWith(candidate.url))
          const name = asset ? basename(path) : ''
          if (!asset || !asset.files.test(name) || !existsSync(asset.dir + name)) return next()
          response.setHeader('Content-Type', asset.type)
          response.setHeader('Cache-Control', 'public, max-age=31536000, immutable')
          response.setHeader('ETag', `"${asset.url}"`)
          response.end(readFileSync(asset.dir + name))
        })
      },
      writeBundle(options) {
        for (const asset of emojiAssets) {
          const target = `${options.dir}${asset.url}`
          mkdirSync(target, { recursive: true })
          cpSync(asset.dir, target, { recursive: true, filter: (source) => source === asset.dir || asset.files.test(basename(source)) })
        }
      },
    },
    {
      name: 'orbit-build-manifest',
      generateBundle() {
        this.emitFile({
          type: 'asset',
          fileName: 'orbit-build.json',
          source: `${JSON.stringify({ contract: openapi.info.version, revision: buildRevision, csp_script_hash: cspScriptHash }, null, 2)}\n`,
        })
      },
    },
  ],
  server: {
    port: 8888,
    allowedHosts: ['.ts.net'],
    proxy: {
      '/.well-known/oauth-': {
        target: process.env.VITE_API_PROXY ?? 'http://127.0.0.1:8080',
      },
      '^/oauth/(authorize|token|register|revoke)(\\?|$)': {
        target: process.env.VITE_API_PROXY ?? 'http://127.0.0.1:8080',
      },
      '/mcp': {
        target: process.env.VITE_API_PROXY ?? 'http://127.0.0.1:8080',
      },
      '/api': {
        target: process.env.VITE_API_PROXY ?? 'http://127.0.0.1:8080',
        ws: true,
      },
    },
  },
})
