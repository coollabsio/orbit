import { expect, test } from 'bun:test'

// Sonner's own styles arrive as a runtime <style> tag that the production CSP (`style-src 'self'`) blocks, which left
// toasts unstyled at the top-left of the page. Only the bundled stylesheet import keeps them styled in production.
test('the toaster bundles sonner styles instead of relying on runtime style injection', async () => {
  const source = await Bun.file(new URL('./sonner.tsx', import.meta.url)).text()
  expect(source).toContain('import "sonner/dist/styles.css"')
})
