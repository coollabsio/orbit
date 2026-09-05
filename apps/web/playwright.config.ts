import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  timeout: 90_000,
  use: {
    baseURL: 'http://127.0.0.1:8888',
    permissions: ['clipboard-read', 'clipboard-write'],
    trace: 'retain-on-failure',
    launchOptions: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE
      ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE }
      : undefined,
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    command: 'VITE_API_PROXY=http://127.0.0.1:18080 bun run dev -- --host 127.0.0.1 --port 8888 --strictPort',
    url: 'http://127.0.0.1:8888',
    reuseExistingServer: false,
  },
})
