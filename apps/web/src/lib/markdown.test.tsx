import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { CustomEmojiContext } from './customEmojiContext'
import { InternalLinkContext } from './internalLinkContext'
import { renderMarkdownBlocks } from './markdown'

function preview(source: string) {
  return render(<div>{renderMarkdownBlocks(source, 'test')}</div>)
}

/** With the rules of chat (Discord). */
function chatPreview(source: string) {
  return render(<div>{renderMarkdownBlocks(source, 'test', [], { chat: true })}</div>)
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

test('in chat ||text|| is a spoiler with inline markdown inside; escaped or in code it stays text', () => {
  const view = chatPreview('a ||hidden **bold**|| b \\|\\|shown\\|\\| `||code||`')
  const spoilers = view.container.querySelectorAll('[data-slot="spoiler"]')

  expect(spoilers).toHaveLength(1)
  expect(spoilers[0].textContent).toBe('hidden bold')
  expect(spoilers[0].querySelector('strong')?.textContent).toBe('bold')
  expect(view.container.textContent).toBe('a hidden bold b ||shown|| ||code||')
})

test('a spoiler shows after a click or Enter, and is no button after that', () => {
  const view = chatPreview('||one|| ||two||')
  const [one, two] = view.getAllByRole('button', { name: 'Spoiler' })

  expect(one.hasAttribute('data-revealed')).toBe(false)
  fireEvent.click(one)
  fireEvent.keyDown(two, { key: 'a' })
  expect(one.hasAttribute('data-revealed')).toBe(true)
  expect(two.hasAttribute('data-revealed')).toBe(false)
  fireEvent.keyDown(two, { key: 'Enter' })
  expect(two.hasAttribute('data-revealed')).toBe(true)
  expect(view.queryByRole('button')).toBeNull()
})

test('outside chat || is text, with the markdown between two of them', () => {
  const view = preview('use a || b, or **c** || d')

  expect(view.container.querySelector('[data-slot="spoiler"]')).toBeNull()
  expect(view.container.textContent).toBe('use a || b, or c || d')
  expect(view.container.querySelector('strong')?.textContent).toBe('c')
})

test('a || in a table row separates cells and starts no spoiler', () => {
  const view = chatPreview('| A | B | C |\n| --- | --- | --- |\n| x || y |')

  expect(Array.from(view.container.querySelectorAll('td')).map((cell) => cell.textContent)).toEqual(['x', '', 'y'])
  expect(view.container.querySelector('[data-slot="spoiler"]')).toBeNull()
})

test('-# is a line of subtext, not a list item', () => {
  const view = preview('-# small *note*\n- item\n-#not subtext')
  const subtext = view.container.querySelectorAll('[data-slot="subtext"]')

  expect(subtext).toHaveLength(1)
  expect(subtext[0].textContent).toBe('small note')
  expect(subtext[0].querySelector('em')?.textContent).toBe('note')
  expect(Array.from(view.container.querySelectorAll('li')).map((item) => item.textContent)).toEqual(['item'])
  expect(view.container.textContent).toBe('small noteitem-#not subtext')
})

test('>>> quotes all the text after it; > quotes its own lines only', () => {
  const rest = preview('before\n>>> first\n\n- second\n**third**')
  const quote = rest.container.querySelector('blockquote')!

  expect(Array.from(quote.children).map((line) => line.textContent)).toEqual(['first', '', '- second', 'third'])
  expect(quote.querySelector('strong')?.textContent).toBe('third')
  expect(rest.container.querySelector('li')).toBeNull()
  expect(rest.container.querySelectorAll('blockquote')).toHaveLength(1)

  const bare = preview('>>>\nonly')
  expect(Array.from(bare.container.querySelector('blockquote')!.children).map((line) => line.textContent)).toEqual(['only'])

  const single = preview('> in\nout')
  expect(single.container.querySelector('blockquote')?.textContent).toBe('in')
  expect(single.container.textContent).toBe('inout')
})

test('***text*** is bold italic, and a style can hold another one', () => {
  const both = preview('***both***').container
  expect(both.querySelector('strong > em')?.textContent).toBe('both')
  expect(both.textContent).toBe('both')

  const bold = preview('**bold with *italic* inside**').container
  expect(bold.querySelector('strong')?.textContent).toBe('bold with italic inside')
  expect(bold.querySelector('strong > em')?.textContent).toBe('italic')

  const italic = preview('*italic with **bold** inside*').container
  expect(italic.querySelector('em')?.textContent).toBe('italic with bold inside')
  expect(italic.querySelector('em > strong')?.textContent).toBe('bold')

  const struck = preview('~~**both**~~').container
  expect(struck.querySelector('del > strong')?.textContent).toBe('both')

  const apart = preview('**a** and *b* and **c**').container
  expect(Array.from(apart.querySelectorAll('strong, em')).map((node) => [node.tagName, node.textContent])).toEqual([['STRONG', 'a'], ['EM', 'b'], ['STRONG', 'c']])
})

test('__text__ is underline with the option, also in a list, a quote and a table; without it bold', () => {
  const source = '__a__\n- __b__\n> __c__\n\n| __d__ |\n| --- |\n| __e__ |'
  const chat = render(<div>{renderMarkdownBlocks(source, 'test', [], { chat: true })}</div>).container
  const other = preview(source).container

  expect(Array.from(chat.querySelectorAll('u')).map((node) => node.textContent)).toEqual(['a', 'b', 'c', 'd', 'e'])
  expect(chat.querySelector('strong')).toBeNull()
  expect(Array.from(other.querySelectorAll('strong')).map((node) => node.textContent)).toEqual(['a', 'b', 'c', 'd', 'e'])
  expect(other.querySelector('u')).toBeNull()
  expect(render(<div>{renderMarkdownBlocks('**x**', 'test', [], { chat: true })}</div>).container.querySelector('strong')?.textContent).toBe('x')
})
