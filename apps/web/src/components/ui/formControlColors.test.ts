import { expect, test } from 'bun:test'

test('form controls keep entered text distinct from muted placeholders', async () => {
  const [input, textarea] = await Promise.all([
    Bun.file(new URL('./input.tsx', import.meta.url)).text(),
    Bun.file(new URL('./textarea.tsx', import.meta.url)).text(),
  ])

  for (const control of [input, textarea]) {
    expect(control).toContain('text-foreground')
    expect(control).toContain('placeholder:text-muted-foreground')
  }
})
