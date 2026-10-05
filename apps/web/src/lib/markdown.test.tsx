import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { CustomEmojiContext } from './customEmojiContext'
import { InternalLinkContext } from './internalLinkContext'
import { renderMarkdownBlocks } from './markdown'

function preview(source: string) {
  return render(<div>{renderMarkdownBlocks(source, 'test')}</div>)
}

test('renders checked and unchecked task items with nested mixed lists', () => {
  const view = preview('- [ ] Plan\n  1. First\n  2. Second\n  - Extra\n- [x] Done')
  const outer = view.container.querySelector('ul')!
  const boxes = view.getAllByRole('checkbox')

  expect(outer.children).toHaveLength(2)
  expect(boxes.map((box) => box.getAttribute('aria-checked'))).toEqual(['false', 'true'])
  expect(boxes.every((box) => box.hasAttribute('data-disabled'))).toBe(true)
  expect(outer.children[0].querySelector('ol')?.children).toHaveLength(2)
  expect(outer.children[0].querySelector('ul')?.textContent).toBe('Extra')
})

test('keeps lists together across blank lines and preserves ordered starting numbers', () => {
  const view = preview('- Parent\n\n  3. Third\n  4. Fourth\n\n- Next')
  const outer = view.container.querySelector('ul')!
  const inner = outer.querySelector('ol')!

  expect(outer.children).toHaveLength(2)
  expect(inner.start).toBe(3)
  expect(inner.children).toHaveLength(2)
})

test('renders tables with inline Markdown and horizontal overflow', () => {
  const view = preview('| Name | State |\n| :--- | ---: |\n| **One** | ~~Old~~ |\n| [Two](https://example.com/two) | New |')
  const table = view.getByRole('table')

  expect(table.querySelectorAll('th')).toHaveLength(2)
  expect(table.querySelectorAll('tbody tr')).toHaveLength(2)
  expect(view.getByText('One').tagName).toBe('STRONG')
  expect(view.getByText('Old').tagName).toBe('DEL')
  expect((view.getByRole('link', { name: 'Two' }) as HTMLAnchorElement).href).toBe('https://example.com/two')
  expect(table.parentElement?.classList.contains('overflow-x-auto')).toBe(true)
  expect(table.querySelectorAll('th')[1].dataset.align).toBe('right')
})

test('renders horizontal rules and strikethrough but leaves fenced code raw', () => {
  const view = preview('Before ~~removed~~\n---\nAfter\n* * *\n```text\n- [x] raw\n---\n```')

  expect(view.getByText('removed').tagName).toBe('DEL')
  expect(view.container.querySelectorAll('hr')).toHaveLength(2)
  expect(view.queryByRole('checkbox')).toBeNull()
  expect(view.getByText(/- \[x\] raw/)).toBeTruthy()
})

test('a backslash before punctuation shows the character and starts no markdown', () => {
  const view = preview('\\*not bold\\* \\:warning: \\_x\\_ \\\\ C:\\Users\\me `a\\*b` :warning:')

  expect(view.container.textContent).toBe('*not bold* :warning: _x_ \\ C:\\Users\\me a\\*b ⚠️')
  expect(view.container.querySelector('em, strong')).toBeNull()
})

test('an escaped block marker is a paragraph', () => {
  const view = preview('\\# title\n\\- item')

  expect(view.container.textContent).toBe('# title- item')
  expect(view.queryByRole('heading')).toBeNull()
  expect(view.container.querySelector('li')).toBeNull()
})

test(':name: of a custom emoji is its image; an escaped or unknown name stays text', () => {
  const custom = new Map([['parrot', { name: 'parrot', url: '/emoji/parrot' }]])
  const view = render(<CustomEmojiContext value={custom}>{renderMarkdownBlocks('go :parrot: \\:parrot: :nobody: **:parrot:**', 'test')}</CustomEmojiContext>)
  const images = Array.from(view.container.querySelectorAll('img'))

  expect(images.map((image) => [image.getAttribute('alt'), image.getAttribute('title'), image.getAttribute('src')])).toEqual([
    [':parrot:', ':parrot:', '/emoji/parrot'],
    [':parrot:', ':parrot:', '/emoji/parrot'],
  ])
  expect(view.container.textContent).toBe('go  :parrot: :nobody: ')
  expect(preview(':parrot:').container.querySelector('img')).toBeNull()
})

test('a URL in angle brackets is one plain link without the brackets', () => {
  const view = preview('see <https://x.com/a> now')
  const links = view.getAllByRole('link')

  expect(links.map((link) => [link.textContent, link.getAttribute('href')])).toEqual([['https://x.com/a', 'https://x.com/a']])
  expect(view.container.textContent).toBe('see https://x.com/a now')
})

test('a URL in angle brackets stays text in code and after a backslash; an image tag is still an image', () => {
  const view = preview('`<https://x.com>` \\<https://y.com> <img src="https://z.com/i.png" alt="pic">')

  expect(view.container.querySelector('code')?.textContent).toBe('<https://x.com>')
  expect(view.getAllByRole('link').map((link) => link.textContent)).toEqual(['https://y.com'])
  expect(view.container.textContent).toBe('<https://x.com> <https://y.com> ')
  expect(view.getByRole('img').getAttribute('src')).toBe('https://z.com/i.png')
})

test('a link to this app in angle brackets is a plain link, also when the host draws chips', () => {
  const url = `${window.location.origin}/tasks/ENG-1`
  const view = render(
    <InternalLinkContext value={() => <span data-slot="chip" />}>{renderMarkdownBlocks(`<${url}> ${url}`, 'test')}</InternalLinkContext>,
  )

  expect(view.getAllByRole('link').map((link) => [link.textContent, link.getAttribute('href')])).toEqual([[url, '/tasks/ENG-1']])
  expect(view.container.querySelectorAll('[data-slot="chip"]')).toHaveLength(1)
})
