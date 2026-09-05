import { defineConfig } from 'vite'
import react, { reactCompilerPreset } from '@vitejs/plugin-react'
import babel from '@rolldown/plugin-babel'
import openapi from './src/api/generated/openapi.json' with { type: 'json' }

const buildRevision = process.env.ORBIT_BUILD_REVISION ?? 'development'

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
          source: `${JSON.stringify({ contract: openapi.info.version, revision: buildRevision }, null, 2)}\n`,
        })
      },
    },
  ],
  server: {
    port: 8888,
    proxy: {
      '/api': {
        target: process.env.VITE_API_PROXY ?? 'http://127.0.0.1:8080',
        ws: true,
      },
    },
  },
})
