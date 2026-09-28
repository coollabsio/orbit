import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { TreeGutter } from './TreeGutter'

const guides = (root: HTMLElement) => root.querySelectorAll('[data-slot="tree-guide"]')

test('one guide per level, capped at the deepest indent; the chevron toggles without reaching the row', () => {
  let toggles = 0
  let rowClicks = 0
  const view = render(
    <div onClick={() => { rowClicks += 1 }}>
      <TreeGutter depth={2} hasChildren expanded onToggle={() => { toggles += 1 }} identifier="ORB-7" />
    </div>,
  )
  expect(guides(view.container)).toHaveLength(2)
  const chevron = view.getByRole('button', { name: 'Collapse sub-issues of ORB-7' })
  expect(chevron.getAttribute('aria-expanded')).toBe('true')
  fireEvent.click(chevron)
  expect([toggles, rowClicks]).toEqual([1, 0])
  view.unmount()

  const deep = render(<TreeGutter depth={9} hasChildren={false} expanded onToggle={() => {}} identifier="ORB-9" />)
  expect(guides(deep.container)).toHaveLength(6)
  expect(deep.queryAllByRole('button')).toHaveLength(0)
})
