import { expect, test } from 'bun:test'

test('the shared topbar exposes both portal slots instead of a route switch', async () => {
  const topbar = await Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text()

  expect(topbar).toContain('data-slot="left"')
  expect(topbar).toContain('data-slot="right"')
  expect(topbar).not.toContain('crumbsFor')
  expect(topbar).not.toContain('topbar-actions')
})

test('settings still uses the shared product pane header contract', async () => {
  const [utilities, settings] = await Promise.all([
    Bun.file(new URL('../../styles/utilities.css', import.meta.url)).text(),
    Bun.file(new URL('../../features/settings/SettingsLayout.tsx', import.meta.url)).text(),
  ])

  expect(settings).toContain('pane-header product-pane-header')
  expect(utilities).toContain('.product-pane-header')
  expect(utilities).toContain('min-height: 44px')
  expect(utilities).toContain('.product-pane-header-icon')
})

test('there is exactly one drawer button and it keeps the 18px burger size', async () => {
  const topbar = await Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text()

  expect(topbar).toContain('<Menu size={18} />')
  expect(topbar.match(/aria-label="Menu"/g) ?? []).toHaveLength(1)
})
