import { afterEach, expect, test } from 'bun:test'
import { fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { render } from '@/test/render'
import { resolveTarget, taskRowTarget, useListNavigation, useTaskTarget } from './taskTarget'
import { TaskTargetProvider } from './TaskTargetProvider'

afterEach(() => {
  document.body.innerHTML = ''
})

const visible = new Set(['a', 'b', 'c'])

test('the target is the selection, then the focused row, then the hovered row', () => {
  expect(resolveTarget({ selectedIds: ['b', 'c'], focusedId: 'a', hoveredId: 'a', openTaskId: null, visible })).toEqual(['b', 'c'])
  expect(resolveTarget({ selectedIds: [], focusedId: 'a', hoveredId: 'b', openTaskId: null, visible })).toEqual(['a'])
  expect(resolveTarget({ selectedIds: [], focusedId: null, hoveredId: 'b', openTaskId: null, visible })).toEqual(['b'])
  expect(resolveTarget({ selectedIds: [], focusedId: null, hoveredId: null, openTaskId: null, visible })).toEqual([])
})

test('an open task is the target, whatever the list says', () => {
  expect(resolveTarget({ selectedIds: ['b'], focusedId: 'a', hoveredId: 'c', openTaskId: 'z', visible })).toEqual(['z'])
})

test('a task that left the list is never the target', () => {
  expect(resolveTarget({ selectedIds: ['gone', 'b'], focusedId: null, hoveredId: null, openTaskId: null, visible })).toEqual(['b'])
  expect(resolveTarget({ selectedIds: ['gone'], focusedId: 'gone', hoveredId: 'c', openTaskId: null, visible })).toEqual(['c'])
  expect(resolveTarget({ selectedIds: [], focusedId: 'gone', hoveredId: 'gone', openTaskId: null, visible })).toEqual([])
})

function Row({ id }: { id: string }) {
  return <div role="button" tabIndex={0} {...taskRowTarget(id)}>{id}</div>
}

const opened: string[] = []
function List({ ids }: { ids: string[] }) {
  useListNavigation((id) => void opened.push(id), { selectable: true })
  const target = useTaskTarget()
  return (
    <>
      {ids.map((id) => <Row key={id} id={id} />)}
      <output data-testid="selected">{target.selectedIds.join(',')}</output>
      <button type="button" onClick={(event) => { event.currentTarget.textContent = target.getTargetIds().join(',') }}>target</button>
    </>
  )
}

const mount = (ids = ['a', 'b', 'c']) => render(<TaskTargetProvider openTaskId={null}><List ids={ids} /></TaskTargetProvider>)
const focused = () => document.activeElement?.getAttribute('data-task-id') ?? null
const selected = (view: ReturnType<typeof mount>) => view.getByTestId('selected').textContent

test('J and K move the focus and stop at the ends', async () => {
  mount()
  await userEvent.keyboard('j')
  expect(focused()).toBe('a')
  await userEvent.keyboard('jjj')
  expect(focused()).toBe('c')
  await userEvent.keyboard('k')
  expect(focused()).toBe('b')
  await userEvent.keyboard('{ArrowUp}{ArrowUp}')
  expect(focused()).toBe('a')
})

test('X selects the focused row, Shift+J extends, Mod+A selects all', async () => {
  const view = mount()
  await userEvent.keyboard('jx')
  expect(selected(view)).toBe('a')
  await userEvent.keyboard('{Shift>}j{/Shift}')
  expect(selected(view)).toBe('a,b')
  await userEvent.keyboard('x')
  expect(selected(view)).toBe('a')
  await userEvent.keyboard('{Control>}a{/Control}')
  expect(selected(view)).toBe('a,b,c')
})

test('O opens the focused row', async () => {
  opened.length = 0
  mount()
  await userEvent.keyboard('jjo')
  expect(opened).toEqual(['b'])
})

test('after the focused row leaves the list, J starts at the first row', async () => {
  const view = mount()
  await userEvent.keyboard('jj')
  view.rerender(<TaskTargetProvider openTaskId={null}><List ids={['a', 'c']} /></TaskTargetProvider>)
  await userEvent.keyboard('j')
  expect(focused()).toBe('a')
})

test('moving the pointer onto a row makes it the target and ends the keyboard focus', async () => {
  const view = mount()
  await userEvent.keyboard('j')
  fireEvent.pointerMove(view.getByText('c'), { clientX: 10, clientY: 10 })
  expect(focused()).toBeNull()
  const probe = view.getByRole('button', { name: 'target' })
  fireEvent.click(probe)
  expect(probe.textContent).toBe('c')
})

test('a hovered row that is gone is not the target', () => {
  const view = mount()
  fireEvent.pointerMove(view.getByText('c'), { clientX: 10, clientY: 10 })
  view.rerender(<TaskTargetProvider openTaskId={null}><List ids={['a', 'b']} /></TaskTargetProvider>)
  const probe = view.getByRole('button', { name: 'target' })
  fireEvent.click(probe)
  expect(probe.textContent).toBe('')
})

test('a row stays the target while the pointer is over a dialog, and stops when it moves to the page', () => {
  const view = render(<TaskTargetProvider openTaskId={null}><List ids={['a', 'b']} /><div role="dialog"><span>menu</span></div><p>page</p></TaskTargetProvider>)
  const probe = view.getByRole('button', { name: 'target' })
  fireEvent.pointerMove(view.getByText('b'), { clientX: 10, clientY: 10 })
  fireEvent.pointerMove(view.getByText('menu'), { clientX: 20, clientY: 20 })
  fireEvent.click(probe)
  expect(probe.textContent).toBe('b')
  fireEvent.pointerMove(view.getByText('page'), { clientX: 30, clientY: 30 })
  fireEvent.click(probe)
  expect(probe.textContent).toBe('')
})

test('after a scroll the row now under the still pointer is the target', () => {
  const view = mount()
  const probe = view.getByRole('button', { name: 'target' })
  fireEvent.pointerMove(view.getByText('a'), { clientX: 10, clientY: 10 })
  const original = document.elementFromPoint
  document.elementFromPoint = () => view.getByText('b')
  try {
    fireEvent.scroll(document)
  } finally {
    document.elementFromPoint = original
  }
  fireEvent.click(probe)
  expect(probe.textContent).toBe('b')
})

test('the focused row stays the target while the focus is in a dialog, and not after it goes elsewhere', () => {
  const view = render(<TaskTargetProvider openTaskId={null}><List ids={['a', 'b']} /><div role="dialog"><input aria-label="menu" /></div><input aria-label="page" /></TaskTargetProvider>)
  const probe = view.getByRole('button', { name: 'target' })
  view.getByText('a').focus()
  // the command menu reads the target as it opens, with the row still focused
  fireEvent.click(probe)
  view.getByLabelText('menu').focus()
  fireEvent.click(probe)
  expect(probe.textContent).toBe('a')
  view.getByLabelText('page').focus()
  fireEvent.click(probe)
  expect(probe.textContent).toBe('')
})
