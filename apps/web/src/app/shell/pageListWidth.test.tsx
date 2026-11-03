import { afterEach, expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { PAGE_LIST_WIDTH, SidebarResizeHandle, usePageListWidth } from './pageListWidth'

function Sidebar() {
  const [width, setWidth] = usePageListWidth()
  return <SidebarResizeHandle width={width} onResize={setWidth} onDragging={() => {}} />
}

afterEach(() => localStorage.clear())

test('the arrow keys resize the sidebar within its limits and the width is saved', () => {
  localStorage.setItem('orbit:page_list_width', '470')
  const view = render(<Sidebar />)
  const handle = view.getByRole('separator', { name: 'Resize sidebar' })
  expect(handle.getAttribute('aria-valuenow')).toBe('470')

  fireEvent.keyDown(handle, { key: 'ArrowRight' })
  expect(handle.getAttribute('aria-valuenow')).toBe(String(PAGE_LIST_WIDTH.max))
  expect(localStorage.getItem('orbit:page_list_width')).toBe(String(PAGE_LIST_WIDTH.max))

  fireEvent.doubleClick(handle)
  fireEvent.keyDown(handle, { key: 'ArrowLeft' })
  expect(handle.getAttribute('aria-valuenow')).toBe(String(PAGE_LIST_WIDTH.default - 16))
})

test('a broken saved width falls back to the default', () => {
  localStorage.setItem('orbit:page_list_width', 'wide')
  const view = render(<Sidebar />)
  expect(view.getByRole('separator').getAttribute('aria-valuenow')).toBe(String(PAGE_LIST_WIDTH.default))
})
