import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { RichTextView } from './RichTextView'

const everything = {
  type: 'doc',
  content: [
    { type: 'heading', attrs: { level: 2 }, content: [{ type: 'text', text: 'Scope' }] },
    {
      type: 'paragraph',
      content: [
        { type: 'text', text: 'bold', marks: [{ type: 'bold' }] },
        { type: 'text', text: 'italic', marks: [{ type: 'italic' }] },
        { type: 'text', text: 'gone', marks: [{ type: 'strike' }] },
        { type: 'text', text: 'snippet', marks: [{ type: 'code' }] },
        { type: 'text', text: 'under', marks: [{ type: 'underline' }] },
        { type: 'text', text: 'site', marks: [{ type: 'link', attrs: { href: 'https://example.com' } }] },
      ],
    },
    { type: 'bulletList', content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'one' }] }] }] },
    { type: 'orderedList', content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'two' }] }] }] },
    { type: 'taskList', content: [{ type: 'taskItem', attrs: { checked: true }, content: [{ type: 'paragraph', content: [{ type: 'text', text: 'done' }] }] }] },
    { type: 'blockquote', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'quoted' }] }] },
    { type: 'codeBlock', attrs: { language: 'rust' }, content: [{ type: 'text', text: 'fn main() {}' }] },
    { type: 'horizontalRule' },
    {
      type: 'paragraph',
      content: [
        { type: 'mention', attrs: { id: 'user-1', label: 'Ada Lovelace' } },
        { type: 'taskMention', attrs: { id: 'task-1', identifier: 'ORB-12' } },
      ],
    },
  ],
}

test('every allowlisted node and mark renders without an editor instance', () => {
  const view = render(<RichTextView document={everything} />)

  expect(view.getByRole('heading', { level: 2 }).textContent).toBe('Scope')
  expect(view.container.querySelector('strong')?.textContent).toBe('bold')
  expect(view.container.querySelector('em')?.textContent).toBe('italic')
  expect(view.container.querySelector('s')?.textContent).toBe('gone')
  expect(view.container.querySelector('code')?.textContent).toBe('snippet')
  expect(view.container.querySelector('u')?.textContent).toBe('under')
  expect(view.container.querySelectorAll('ul').length).toBeGreaterThanOrEqual(1)
  expect(view.container.querySelector('ol')?.textContent).toBe('two')
  expect(view.container.querySelector('blockquote')?.textContent).toBe('quoted')
  expect(view.container.querySelector('pre')?.textContent).toBe('fn main() {}')
  expect(view.container.querySelector('hr')).toBeTruthy()
})

test('links open safely and never carry a javascript scheme', () => {
  const view = render(
    <RichTextView
      document={{
        type: 'doc',
        content: [
          {
            type: 'paragraph',
            content: [
              { type: 'text', text: 'safe', marks: [{ type: 'link', attrs: { href: 'https://example.com' } }] },
              { type: 'text', text: 'unsafe', marks: [{ type: 'link', attrs: { href: 'javascript:alert(1)' } }] },
            ],
          },
        ],
      }}
    />,
  )

  const links = view.getAllByRole('link') as HTMLAnchorElement[]
  expect(links[0].getAttribute('href')).toBe('https://example.com')
  expect(links[0].rel).toContain('noreferrer')
  expect(links[0].target).toBe('_blank')
  expect(links[1].getAttribute('href')).toBe('#')
})

test('a person mention shows its cached label and a task chip shows its identifier', () => {
  const view = render(
    <RichTextView
      document={{
        type: 'doc',
        content: [
          {
            type: 'paragraph',
            content: [
              { type: 'mention', attrs: { id: 'user-1', label: 'Ada Lovelace' } },
              { type: 'taskMention', attrs: { id: 'task-1', identifier: 'ORB-12' } },
            ],
          },
        ],
      }}
    />,
  )

  expect(view.getByText('@Ada Lovelace')).toBeTruthy()
  expect(view.getByText('ORB-12')).toBeTruthy()
})

test('a resolved chip shows the live title and strikes through when cancelled', () => {
  const view = render(
    <RichTextView
      document={{
        type: 'doc',
        content: [
          {
            type: 'paragraph',
            content: [{ type: 'taskMention', attrs: { id: 'task-1', identifier: 'ORB-12' } }],
          },
        ],
      }}
      chips={() => ({ title: 'Renamed later', statusCategory: 'cancelled' })}
    />,
  )

  expect(view.getByText('Renamed later')).toBeTruthy()
  expect(view.container.querySelector('.editor-chip')?.getAttribute('data-cancelled')).toBe('true')
})

test('junk input renders an empty view instead of throwing', () => {
  expect(render(<RichTextView document={null} />).container.textContent).toBe('')
  expect(render(<RichTextView document={'nope'} />).container.textContent).toBe('')
})
