import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { useState, type ReactNode } from 'react'
import { TopbarSlot, TopbarSlotProvider, useTopbarSlotTarget } from './TopbarSlot'

function Chrome() {
  const left = useTopbarSlotTarget('left')
  const right = useTopbarSlotTarget('right')
  return (
    <header>
      <div data-testid="left-target" ref={left} />
      <div data-testid="right-target" ref={right} />
    </header>
  )
}

function Page() {
  return (
    <>
      <TopbarSlot side="left"><span>Orbit › ORB-12</span></TopbarSlot>
      <TopbarSlot side="right"><button type="button">New task</button></TopbarSlot>
    </>
  )
}

function Shell({ children }: { children: ReactNode }) {
  return <TopbarSlotProvider>{children}</TopbarSlotProvider>
}

test('page content is portalled into the shell slot containers', () => {
  const view = render(<Shell><Chrome /><Page /></Shell>)

  expect(view.getByTestId('left-target').textContent).toBe('Orbit › ORB-12')
  expect(view.getByTestId('right-target').querySelector('button')?.textContent).toBe('New task')
})

test('a page that renders before the chrome still lands in the slot', () => {
  const view = render(<Shell><Page /><Chrome /></Shell>)

  expect(view.getByTestId('left-target').textContent).toBe('Orbit › ORB-12')
  expect(view.getByTestId('right-target').querySelector('button')?.textContent).toBe('New task')
})

test('unmounting the chrome releases the target instead of throwing', () => {
  function Toggling() {
    const [mounted, setMounted] = useState(true)
    return (
      <Shell>
        {mounted ? <Chrome /> : null}
        <Page />
        <button type="button" onClick={() => setMounted(false)}>Hide chrome</button>
      </Shell>
    )
  }
  const view = render(<Toggling />)
  expect(view.getByTestId('left-target').textContent).toBe('Orbit › ORB-12')

  fireEvent.click(view.getByRole('button', { name: 'Hide chrome' }))
  expect(view.queryByTestId('left-target')).toBeNull()
  expect(view.queryByText('Orbit › ORB-12')).toBeNull()
})

test('a slot without a provider renders nothing and does not throw', () => {
  const view = render(<Page />)

  expect(view.queryByText('Orbit › ORB-12')).toBeNull()
  expect(view.queryByRole('button', { name: 'New task' })).toBeNull()
})
