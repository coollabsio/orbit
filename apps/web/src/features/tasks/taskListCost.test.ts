import { expect, test } from 'bun:test'

const read = (path: string) => Bun.file(new URL(path, import.meta.url)).text()

test('list and board rows never import the editor', async () => {
  for (const file of ['./components/TaskList.tsx', './components/TaskBoard.tsx', './components/TaskRow.tsx']) {
    const source = await read(file)
    expect(source).not.toContain('RichTextEditor')
    expect(source).not.toContain('RichTextView')
    expect(source).not.toContain('@tiptap/')
  }
})

test('the stale markdown-era comment styles are gone from tasks.css', async () => {
  const css = await read('./tasks.css')

  for (const stale of ['.fc-md-p', '.fc-md-blank', '.fc-md-heading', '.fc-md-quote', '.fc-md-list', '.tasks-mention-list']) {
    expect(css).not.toContain(stale)
  }
})

test('the comment text container defers formatting to the editor stylesheet', async () => {
  const css = await read('./tasks.css')
  const rule = css.match(/\.tasks-comment-text\s*\{([^}]*)\}/)?.[1]

  expect(rule).toContain('overflow-wrap: anywhere')
})

test('the composer keeps its stacked layout without a textarea rule', async () => {
  const css = await read('./tasks.css')
  const rule = css.match(/\.tasks-native-composer\s*\{([^}]*)\}/)?.[1]

  expect(rule).toContain('position: relative')
  expect(css).not.toContain('.tasks-native-composer textarea')
})

test('no task feature file still reads the removed markdown fields', async () => {
  const glob = new Bun.Glob('**/*.{ts,tsx}')
  const offenders: string[] = []
  for await (const file of glob.scan({ cwd: new URL('.', import.meta.url).pathname })) {
    if (file.endsWith('.test.ts') || file.endsWith('.test.tsx')) continue
    const source = await Bun.file(new URL(`./${file}`, import.meta.url)).text()
    if (/\btask\.description\b|\bcomment\.body\b|mentionedUserIds|mentioned_user_ids/.test(source)) {
      offenders.push(file)
    }
  }

  expect(offenders).toEqual([])
})

test('list search matches the server-derived description text, never the document', async () => {
  const source = await read('./tasksLib.ts')

  expect(source).toContain('t.descriptionText')
  expect(source).not.toContain('descriptionJson')
})
