import { afterEach, describe, expect, mock, test } from 'bun:test'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { CommentsPanel } from './CommentsPanel'

afterEach(cleanup)

function renderPanel(overrides: Partial<Parameters<typeof CommentsPanel>[0]> = {}) {
  const props = {
    filter: 'open' as const,
    onFilterChange: mock(() => {}),
    openCount: 0,
    resolvedCount: 0,
    loading: false,
    listRef: mock(() => {}),
    onClose: mock(() => {}),
    ...overrides,
  }
  return { view: render(<CommentsPanel {...props} />), props }
}

describe('CommentsPanel', () => {
  test('explains how to start a thread when nothing is open', () => {
    const { view } = renderPanel()
    expect(view.getByRole('complementary', { name: 'Comments' })).toBeTruthy()
    expect(view.getByText('No open comments')).toBeTruthy()
    expect(view.getByText('Select text and click Comment to start a thread.')).toBeTruthy()
  })

  test('shows the loading state', () => {
    const { view } = renderPanel({ loading: true })
    expect(view.getByText('Loading comments…')).toBeTruthy()
  })

  test('shows counts per tab and hands the list container to the editor when there are threads', () => {
    const { view, props } = renderPanel({ openCount: 2, resolvedCount: 1 })
    expect(view.queryByText(/No open comments/)).toBeNull()
    expect(view.getByRole('tab', { name: /Open\s*2/ })).toBeTruthy()
    expect(view.getByRole('tab', { name: /Resolved\s*1/ })).toBeTruthy()
    expect(view.getByRole('tab', { name: /Open/ }).getAttribute('aria-selected')).toBe('true')
    expect(props.listRef).toHaveBeenCalled()
  })

  test('switches to resolved threads and closes', () => {
    const { view, props } = renderPanel({ openCount: 1 })
    fireEvent.click(view.getByRole('tab', { name: /Resolved/ }))
    expect(props.onFilterChange).toHaveBeenCalledWith('resolved')
    fireEvent.click(view.getByRole('button', { name: 'Close comments' }))
    expect(props.onClose).toHaveBeenCalled()
  })

  test('the resolved tab has its own empty state', () => {
    const { view } = renderPanel({ filter: 'resolved', openCount: 3 })
    expect(view.getByText('No resolved comments')).toBeTruthy()
    expect(view.container.querySelector('[data-comments-empty="resolved"]')).toBeTruthy()
  })
})
