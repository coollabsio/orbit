import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { Dropdown } from './Dropdown'

test('dropdown escapes clipping ancestors and keeps interactions inside the menu open', () => {
  const view = render(<div style={{ overflow: 'hidden', height: 40 }}><Dropdown
    className="custom-dropdown"
    trigger={() => <button>Open menu</button>}
  >{(close) => <><button>Keep open</button><button onClick={close}>Choose</button></>}</Dropdown></div>)
  fireEvent.click(view.getByText('Open menu'))
  const option = view.getByText('Keep open')
  expect(view.container.contains(option)).toBe(false)
  expect(option.closest('.custom-dropdown')).not.toBeNull()
  fireEvent.pointerDown(option)
  fireEvent.click(option)
  expect(view.queryByText('Choose')).not.toBeNull()
  fireEvent.click(view.getByText('Choose'))
  expect(view.queryByText('Choose')).toBeNull()
  fireEvent.click(view.getByText('Open menu'))
  fireEvent.keyDown(document, { key: 'Escape' })
  expect(view.queryByText('Choose')).toBeNull()
  fireEvent.click(view.getByText('Open menu'))
  fireEvent.pointerDown(document.body)
  expect(view.queryByText('Choose')).toBeNull()
})
