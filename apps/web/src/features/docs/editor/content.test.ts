import { describe, expect, test } from 'bun:test'
import { contentEquals, internalPageLinkId, isPageFileUrl, isSafeFileUrl, isSafeLinkHref, isTaskAttachmentUrl, linkMentions, normalizeLinkHref, pastedFilesToCopy, taskAttachmentUrl, toEditorContent, toStoredContent } from './content'

const TYPES = new Set(['paragraph', 'heading', 'table', 'page'])
const NUL = String.fromCharCode(0)
const TAB = String.fromCharCode(9)

function link(href: string, text = 'x') {
  return { type: 'link', href, content: [{ type: 'text', text, styles: {} }] }
}

describe('link policy', () => {
  test('only absolute http(s) and mailto hrefs are safe', () => {
    expect(isSafeLinkHref('https://example.com')).toBe(true)
    expect(isSafeLinkHref('HTTP://example.com/a?b#c')).toBe(true)
    expect(isSafeLinkHref('mailto:a@b.co')).toBe(true)
    for (const href of ['javascript:alert(1)', ` java${TAB}script:alert(1)`, `java${NUL}script:x`, 'data:text/html,x', 'vbscript:x', 'ftp://x', 'tel:1', '/docs/1', '#top', 'https:/x', '', null, 3]) {
      expect(isSafeLinkHref(href)).toBe(false)
    }
  })

  test('exact in-app page links /docs/<uuid> are safe; other relative paths are not', () => {
    const id = '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b'
    expect(isSafeLinkHref(`/docs/${id}`)).toBe(true)
    expect(normalizeLinkHref(`/docs/${id}`)).toBe(`/docs/${id}`)
    expect(internalPageLinkId(`/docs/${id.toUpperCase()}`)).toBe(id)
    for (const href of [
      `/docs/${id}/x`,
      `/docs/${id}?a=1`,
      `/docs/${id}#h`,
      `//docs/${id}`,
      `/docs//${id}`,
      `/docs/${id} `,
      `${TAB}/docs/${id}`,
      `/docs/${id.slice(1)}`,
      '/docs/trash',
      '/docs/import',
      `/tasks/${id}`,
      `javascript:/docs/${id}`,
      `https:/docs/${id}`,
    ]) {
      expect(isSafeLinkHref(href)).toBe(false)
      expect(normalizeLinkHref(href)).toBeNull()
      expect(internalPageLinkId(href)).toBeNull()
    }
  })

  test('normalizeLinkHref upgrades bare hosts and rejects everything else', () => {
    expect(normalizeLinkHref(' https://a.io ')).toBe('https://a.io')
    expect(normalizeLinkHref('example.com/path')).toBe('https://example.com/path')
    expect(normalizeLinkHref('example.com:8080/x')).toBe('https://example.com:8080/x')
    expect(normalizeLinkHref('javascript:alert(1)')).toBeNull()
    expect(normalizeLinkHref('JavaScript:1.2')).toBeNull()
    expect(normalizeLinkHref('localhost:3000')).toBeNull()
    expect(normalizeLinkHref('/relative')).toBeNull()
    expect(normalizeLinkHref('#hash')).toBeNull()
    expect(normalizeLinkHref('word')).toBeNull()
    expect(normalizeLinkHref(undefined)).toBeNull()
  })
})

describe('toEditorContent', () => {
  test('empty or invalid content becomes undefined so BlockNote creates its own empty paragraph', () => {
    expect(toEditorContent([], TYPES)).toBeUndefined()
    expect(toEditorContent(null, TYPES)).toBeUndefined()
    expect(toEditorContent({ type: 'paragraph' }, TYPES)).toBeUndefined()
    expect(toEditorContent([{ type: 'image', props: { url: 'x' } }, 'junk', null], TYPES)).toBeUndefined()
  })

  test('drops unknown block types at every nesting level and keeps the rest untouched', () => {
    const content = [
      { id: 'a', type: 'paragraph', content: [], children: [{ id: 'b', type: 'video' }, { id: 'c', type: 'heading', children: [] }] },
      { id: 'd', type: 'file' },
      { id: 'e', type: 'page', props: { pageId: 'p1' } },
    ]
    expect(toEditorContent(content, TYPES)).toEqual([
      { id: 'a', type: 'paragraph', content: [], children: [{ id: 'c', type: 'heading', children: [] }] },
      { id: 'e', type: 'page', props: { pageId: 'p1' } },
    ])
  })

  test('unwraps unsafe links into plain text anywhere in the document, including table cells', () => {
    const content = [
      { type: 'paragraph', content: [link('javascript:alert(1)', 'evil'), { type: 'text', text: ' ok', styles: {} }, link('example.com')] },
      {
        type: 'table',
        content: { type: 'tableContent', rows: [{ cells: [{ type: 'tableCell', content: [link('data:text/html,x', 'cell')] }] }] },
      },
    ]
    expect(toEditorContent(content, TYPES)).toEqual([
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'evil', styles: {} },
          { type: 'text', text: ' ok', styles: {} },
          { type: 'link', href: 'https://example.com', content: [{ type: 'text', text: 'x', styles: {} }] },
        ],
      },
      {
        type: 'table',
        content: { type: 'tableContent', rows: [{ cells: [{ type: 'tableCell', content: [{ type: 'text', text: 'cell', styles: {} }] }] }] },
      },
    ])
  })
})

test('toStoredContent sanitizes links without mutating the editor document', () => {
  const document = [{ id: 'a', type: 'paragraph', content: [link('javascript:x')], children: [] }]
  const stored = toStoredContent(document)
  expect(stored).toEqual([{ id: 'a', type: 'paragraph', content: [{ type: 'text', text: 'x', styles: {} }], children: [] }])
  expect(document[0].content[0].type).toBe('link')
})

test('contentEquals ignores key order and undefined values but not data', () => {
  expect(contentEquals([{ a: 1, b: { c: [1, 2] } }], [{ b: { c: [1, 2] }, a: 1, d: undefined }])).toBe(true)
  expect(contentEquals([{ a: 1 }], [{ a: 2 }])).toBe(false)
  expect(contentEquals([{ c: [1, 2] }], [{ c: [2, 1] }])).toBe(false)
  expect(contentEquals([], [])).toBe(true)
})

describe('file block URLs', () => {
  const FILE = '/api/v1/workspaces/01a0d778-64cb-7422-a4df-c7eae799677f/pages/01a0d779-0000-7000-8000-000000000001/files/01a0d779-0000-7000-8000-000000000002'

  test('uploaded page files, http(s) URLs and empty (pending upload) URLs are allowed', () => {
    expect(isPageFileUrl(FILE)).toBe(true)
    for (const url of [FILE, '', 'https://example.com/a.png', 'http://example.com/a.pdf']) expect(isSafeFileUrl(url)).toBe(true)
    for (const url of [
      'javascript:alert(1)',
      ` java${TAB}script:alert(1)`,
      'data:image/png;base64,AAAA',
      'blob:https://x/1',
      '/api/v1/auth/me',
      `${FILE}?x=1`,
      `${FILE}/..`,
      FILE.toUpperCase(),
      '//evil.example/x.png',
      'https:/x',
      null,
    ]) {
      expect(isSafeFileUrl(url)).toBe(false)
    }
  })

  test('unsafe image/file URLs are blanked on load and before saving, nested blocks included', () => {
    const types = new Set(['paragraph', 'image', 'file'])
    const content = [
      { id: 'a', type: 'image', props: { url: FILE, caption: 'ok' } },
      { id: 'b', type: 'file', props: { url: 'javascript:alert(1)', name: 'x' } },
      { id: 'c', type: 'paragraph', children: [{ id: 'd', type: 'image', props: { url: 'data:image/png;base64,AAAA' } }] },
    ]
    for (const result of [toEditorContent(content, types)!, toStoredContent(content)]) {
      const blocks = result as { props?: { url: string }; children?: { props: { url: string } }[] }[]
      expect(blocks[0].props?.url).toBe(FILE)
      expect(blocks[1].props?.url).toBe('')
      expect(blocks[2].children?.[0].props.url).toBe('')
    }
  })
})

describe('task attachment urls', () => {
  const ws = '0199a0b0-0000-7000-8000-000000000001'
  const task = '0199a0b0-0000-7000-8000-000000000002'
  const file = '0199a0b0-0000-7000-8000-000000000003'
  const scope = { workspaceId: ws, taskId: task }

  test('taskAttachmentUrl is the download path', () => {
    expect(taskAttachmentUrl(ws, task, file)).toBe(`/api/v1/workspaces/${ws}/tasks/${task}/attachments/${file}/download`)
  })

  test('attachments of tasks in the same workspace are safe for a task', () => {
    expect(isTaskAttachmentUrl(taskAttachmentUrl(ws, task, file), scope)).toBe(true)
    // copied from another task of the workspace
    expect(isTaskAttachmentUrl(taskAttachmentUrl(ws, file, file), scope)).toBe(true)
    expect(isTaskAttachmentUrl(taskAttachmentUrl(file, task, file), scope)).toBe(false)
    expect(isTaskAttachmentUrl(`/api/v1/workspaces/${ws}/pages/${task}/files/${file}`, scope)).toBe(false)
    expect(isSafeFileUrl(taskAttachmentUrl(ws, task, file), scope)).toBe(true)
    expect(isSafeFileUrl(`/api/v1/workspaces/${ws}/pages/${task}/files/${file}`, scope)).toBe(false)
  })

  test('task content keeps its own attachments when loaded and stored', () => {
    const url = taskAttachmentUrl(ws, task, file)
    const blocks = [{ type: 'image', props: { url } }]
    expect(toEditorContent(blocks, new Set(['image']), scope)?.[0]).toEqual({ type: 'image', props: { url } })
    expect(toStoredContent(blocks, scope)[0]).toEqual({ type: 'image', props: { url } })
    expect(toStoredContent(blocks)[0]).toEqual({ type: 'image', props: { url: '' } })
  })
})

describe('pastedFilesToCopy', () => {
  const ws = '0199a0b0-0000-7000-8000-000000000001'
  const task = '0199a0b0-0000-7000-8000-000000000002'
  const other = '0199a0b0-0000-7000-8000-000000000003'
  const file = '0199a0b0-0000-7000-8000-000000000004'
  const scope = { workspaceId: ws, taskId: task }
  const block = (id: string, url: string, children: unknown[] = []) => ({ id, type: 'image', props: { url, name: 'shot.png' }, children })

  test('files of another task or of a page are copied; own files, links and earlier blocks are not', () => {
    const fromTask = taskAttachmentUrl(ws, other, file)
    const fromPage = `/api/v1/workspaces/${ws}/pages/${other}/files/${file}`
    const document = [
      block('kept', taskAttachmentUrl(ws, other, file)),
      block('own', taskAttachmentUrl(ws, task, file)),
      block('web', 'https://example.com/a.png'),
      { id: 'p', type: 'paragraph', children: [block('task', fromTask), block('page', fromPage)] },
    ]
    expect(pastedFilesToCopy(document, new Set(['kept']), scope)).toEqual([
      { id: 'task', url: fromTask, name: 'shot.png' },
      { id: 'page', url: fromPage, name: 'shot.png' },
    ])
  })
})

describe('linkMentions', () => {
  const ada = { id: 'u-ada', name: 'Ada Lovelace' }
  const text = (value: string, styles = {}) => ({ type: 'text', text: value, styles })
  const paragraph = (...content: unknown[]) => [{ type: 'paragraph', content, children: [] }]

  test('an exact member name after @ becomes a mention', () => {
    expect(linkMentions(paragraph(text('hi @Ada Lovelace, look')), [ada, { id: 'u-ad', name: 'Ada' }])).toEqual(
      paragraph(text('hi '), { type: 'mention', props: { userId: 'u-ada', name: 'Ada Lovelace' } }, text(', look')),
    )
  })

  test('two members with the name, no word boundary, or code stay text', () => {
    const twins = [{ id: 'a', name: 'Ada' }, { id: 'b', name: 'Ada' }]
    expect(linkMentions(paragraph(text('hi @Ada')), twins)).toEqual(paragraph(text('hi @Ada')))
    expect(linkMentions(paragraph(text('@Adam')), [{ id: 'a', name: 'Ada' }])).toEqual(paragraph(text('@Adam')))
    expect(linkMentions(paragraph(text('mail@Ada')), [{ id: 'a', name: 'Ada' }])).toEqual(paragraph(text('mail@Ada')))
    expect(linkMentions(paragraph(text('@Ada', { code: true })), [{ id: 'a', name: 'Ada' }])).toEqual(paragraph(text('@Ada', { code: true })))
  })
})
