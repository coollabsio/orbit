import { afterEach, expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { NAV_WIDTH, PAGE_LIST_WIDTH, SidebarResizeHandle, useSidebarWidth, type SidebarWidthRange } from './pageListWidth'

function Sidebar({ range = PAGE_LIST_WIDTH }: { range?: SidebarWidthRange }) {
  const [width, setWidth] = useSidebarWidth(range)
  return <SidebarResizeHandle width={width} range={range} onResize={setWidth} onDragging={() => {}} />
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

test('the sidebar of pages without a list keeps its own width', () => {
  localStorage.setItem('orbit:page_list_width', '400')
  const view = render(<Sidebar range={NAV_WIDTH} />)
  const handle = view.getByRole('separator', { name: 'Resize sidebar' })
  expect(handle.getAttribute('aria-valuenow')).toBe('224')

  fireEvent.keyDown(handle, { key: 'ArrowRight' })
  expect(localStorage.getItem('orbit:sidebar_width')).toBe('240')
  expect(localStorage.getItem('orbit:page_list_width')).toBe('400')
})
