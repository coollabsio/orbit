import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { MentionList } from './MentionList'
import type { MentionItem } from './extensions/mentionSuggestion'

const items: MentionItem[] = [
  { kind: 'user', id: 'u1', label: 'Ada Lovelace', handle: 'ada' },
  { kind: 'user', id: 'u2', label: 'Grace Hopper', handle: 'grace' },
  { kind: 'task', id: 't1', identifier: 'ORB-12', title: 'Ship the editor' },
]

test('people and issues are grouped under headers', () => {
  const view = render(<MentionList items={items} activeIndex={0} onSelect={() => {}} onHover={() => {}} />)

  const headers = Array.from(view.container.querySelectorAll('.editor-mention-group')).map((node) => node.textContent)
  expect(headers).toEqual(['People', 'Issues'])
  expect(view.getByText('Ada Lovelace')).toBeTruthy()
  expect(view.getByText('ORB-12')).toBeTruthy()
  expect(view.getByText('Ship the editor')).toBeTruthy()
})

test('the active option is marked and selecting one reports the item, not the index', () => {
  const onSelect = mock((_item: MentionItem) => {})
  const view = render(<MentionList items={items} activeIndex={2} onSelect={onSelect} onHover={() => {}} />)

  const options = view.getAllByRole('option')
  expect(options[2].getAttribute('aria-selected')).toBe('true')
  expect(options[0].getAttribute('aria-selected')).toBe('false')

  fireEvent.mouseDown(options[2])
  expect(onSelect).toHaveBeenCalledWith(items[2])
})

test('hovering an option reports its flat index so keyboard and mouse stay in sync', () => {
  const onHover = mock((_index: number) => {})
  const view = render(<MentionList items={items} activeIndex={0} onSelect={() => {}} onHover={onHover} />)

  fireEvent.mouseEnter(view.getAllByRole('option')[1])
  expect(onHover).toHaveBeenCalledWith(1)
})

test('an empty list renders nothing', () => {
  const view = render(<MentionList items={[]} activeIndex={0} onSelect={() => {}} onHover={() => {}} />)
  expect(view.container.firstChild).toBeNull()
})

test('the listbox announces itself for assistive technology', () => {
  const view = render(<MentionList items={items} activeIndex={0} onSelect={() => {}} onHover={() => {}} />)
  expect(view.getByRole('listbox').getAttribute('aria-label')).toBe('Mention a person or issue')
})
