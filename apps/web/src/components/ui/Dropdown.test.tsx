import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { Dropdown } from './Dropdown'

// Base UI keeps the closed popup mounted for its exit animation, so we assert
// open/closed through the trigger's aria-expanded rather than DOM removal.
test('dropdown escapes clipping ancestors and keeps interactions inside the menu open', () => {
  const view = render(
    <div style={{ overflow: 'hidden', height: 40 }}>
      <Dropdown className="custom-dropdown" trigger={() => <button>Open menu</button>}>
        {(close) => (
          <>
            <button>Keep open</button>
            <button onClick={close}>Choose</button>
          </>
        )}
      </Dropdown>
    </div>,
  )
  const triggerButton = view.getByRole('button', { name: 'Open menu' })
  const isOpen = () => triggerButton.getAttribute('aria-expanded') === 'true'

  fireEvent.click(triggerButton)
  expect(isOpen()).toBe(true)
  const option = view.getByText('Keep open')
  // Content is portaled out of the clipping ancestor but tagged with our className.
  expect(view.container.contains(option)).toBe(false)
  expect(option.closest('.custom-dropdown')).not.toBeNull()

  // Interacting inside the menu keeps it open.
  fireEvent.pointerDown(option)
  fireEvent.click(option)
  expect(isOpen()).toBe(true)

  // Choosing (calling close) dismisses it.
  fireEvent.click(view.getByText('Choose'))
  expect(isOpen()).toBe(false)

  // Escape dismisses.
  fireEvent.click(triggerButton)
  expect(isOpen()).toBe(true)
  fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' })
  expect(isOpen()).toBe(false)
})
