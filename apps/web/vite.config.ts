import { defineConfig } from 'vite'
import react, { reactCompilerPreset } from '@vitejs/plugin-react'
import babel from '@rolldown/plugin-babel'
import openapi from './src/api/generated/openapi.json' with { type: 'json' }
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'

const buildRevision = process.env.ORBIT_BUILD_REVISION ?? 'development'
const sourceHtml = readFileSync(new URL('./index.html', import.meta.url), 'utf8')
const bootstrapScript = sourceHtml.match(/<script>([\s\S]*?)<\/script>/)?.[1]
if (!bootstrapScript) throw new Error('theme bootstrap script is missing')
const cspScriptHash = `sha256-${createHash('sha256').update(bootstrapScript).digest('base64')}`

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    react(),
    babel({ presets: [reactCompilerPreset()] }),
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
      '/api': {
        target: process.env.VITE_API_PROXY ?? 'http://127.0.0.1:8080',
        ws: true,
      },
    },
  },
})
