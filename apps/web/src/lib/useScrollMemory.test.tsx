import { afterEach, expect, test } from 'bun:test'
import { useRef } from 'react'
import { cleanup, fireEvent } from '@testing-library/react'
import { MemoryRouter, Route, Routes, useNavigate } from 'react-router'
import { render } from '@/test/render'
import { useScrollMemory } from './useScrollMemory'

afterEach(cleanup)

function List() {
  const scroller = useRef<HTMLDivElement>(null)
  useScrollMemory(scroller, 'test-list')
  return <div ref={scroller} data-testid="scroller" />
}

function Nav() {
  const navigate = useNavigate()
  return ['/list', '/list?project=2', '/task'].map((to) => <button key={to} type="button" onClick={() => navigate(to)}>{to}</button>)
}

test('a scroller is back at its position when its page shows again, and each page keeps its own', () => {
  const page = render(
    <MemoryRouter initialEntries={['/list']}>
      <Routes>
        <Route path="list" element={<List />} />
        <Route path="task" element={<p>Task</p>} />
      </Routes>
      <Nav />
    </MemoryRouter>,
  )
  const scroller = () => page.getByTestId('scroller')
  scroller().scrollTop = 120
  fireEvent.scroll(scroller())

  fireEvent.click(page.getByText('/task'))
  fireEvent.click(page.getByText('/list'))
  expect(scroller().scrollTop).toBe(120)

  fireEvent.click(page.getByText('/task'))
  fireEvent.click(page.getByText('/list?project=2'))
  expect(scroller().scrollTop).toBe(0)
})
