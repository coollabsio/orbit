import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { renderMarkdownBlocks } from './markdown'

function preview(source: string) {
  return render(<div>{renderMarkdownBlocks(source, 'test')}</div>)
}

test('renders checked and unchecked task items with nested mixed lists', () => {
  const view = preview('- [ ] Plan\n  1. First\n  2. Second\n  - Extra\n- [x] Done')
  const outer = view.container.querySelector('ul')!
  const boxes = view.getAllByRole('checkbox') as HTMLInputElement[]

  expect(outer.children).toHaveLength(2)
  expect(boxes.map((box) => box.checked)).toEqual([false, true])
  expect(boxes.every((box) => box.disabled)).toBe(true)
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
  expect(table.querySelectorAll('th')[1].classList.contains('text-right')).toBe(true)
})

test('renders horizontal rules and strikethrough but leaves fenced code raw', () => {
  const view = preview('Before ~~removed~~\n---\nAfter\n* * *\n```text\n- [x] raw\n---\n```')

  expect(view.getByText('removed').tagName).toBe('DEL')
  expect(view.container.querySelectorAll('hr')).toHaveLength(2)
  expect(view.queryByRole('checkbox')).toBeNull()
  expect(view.getByText(/- \[x\] raw/)).toBeTruthy()
})
