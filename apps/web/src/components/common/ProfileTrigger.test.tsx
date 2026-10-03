import { afterEach, expect, test } from 'bun:test'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { ProfilePopoverContext, type ProfilePopoverControl } from './profilePopover'
import { ProfileTrigger } from './ProfileTrigger'

afterEach(cleanup)

function setup(trigger: (onRow: () => void) => React.ReactNode) {
  const opened: Array<{ userId: string; anchor: HTMLElement; instant: boolean }> = []
  const prefetched: string[] = []
  let rowActions = 0
  const control: ProfilePopoverControl = {
    open: (userId, anchor, { instant }) => opened.push({ userId, anchor, instant }),
    prefetch: (userId) => prefetched.push(userId),
  }
  const onRow = () => (rowActions += 1)
  const view = render(<ProfilePopoverContext value={control}>{trigger(onRow)}</ProfilePopoverContext>)
  return { view, opened, prefetched, rowActions: () => rowActions }
}

test('a click on a trigger opens the profile and does not reach the row around it', () => {
  const { view, opened, rowActions } = setup((onRow) => (
    <div role="row" onClick={onRow} onKeyDown={(event) => event.key === 'Enter' && onRow()}>
      <ProfileTrigger userId="u1" name="Ada">
        Ada
      </ProfileTrigger>
      <span>the rest of the row</span>
    </div>
  ))
  const trigger = view.getByRole('button', { name: 'Open profile of Ada' })
  fireEvent.click(trigger)
  fireEvent.keyDown(trigger, { key: 'Enter' })
  expect(opened.map((call) => call.userId)).toEqual(['u1'])
  expect(opened[0]!.anchor).toBe(trigger)
  expect(rowActions()).toBe(0)

  fireEvent.click(view.getByText('the rest of the row'))
  expect(rowActions()).toBe(1)
})

test('other events of a trigger still reach the row: its context menu and its long press', () => {
  let menus = 0
  let touches = 0
  const { view } = setup(() => (
    <div onContextMenu={() => (menus += 1)} onTouchStart={() => (touches += 1)}>
      <ProfileTrigger userId="u1" name="Ada">
        Ada
      </ProfileTrigger>
    </div>
  ))
  const trigger = view.getByRole('button')
  fireEvent.contextMenu(trigger)
  fireEvent.touchStart(trigger)
  expect([menus, touches]).toEqual([1, 1])
})

test('the click that ends a long press on a touch screen opens nothing', () => {
  const { view, opened } = setup(() => (
    <ProfileTrigger userId="u1" name="Ada">
      Ada
    </ProfileTrigger>
  ))
  const trigger = view.getByRole('button')
  const event = (type: string, timeStamp: number, init: object = {}) => {
    const created = new MouseEvent(type, { bubbles: true, cancelable: true, detail: 1 })
    Object.defineProperties(created, { timeStamp: { value: timeStamp }, ...Object.fromEntries(Object.entries(init).map(([name, value]) => [name, { value }])) })
    return created
  }
  fireEvent(trigger, event('pointerdown', 1000, { pointerType: 'touch' }))
  fireEvent(trigger, event('click', 1700))
  expect(opened).toEqual([])
  // a tap
  fireEvent(trigger, event('pointerdown', 3000, { pointerType: 'touch' }))
  fireEvent(trigger, event('click', 3080))
  expect(opened.map((call) => call.instant)).toEqual([false])
})

test('a press loads the profile before the click; a keyboard click opens without the entrance motion', () => {
  const { view, opened, prefetched } = setup(() => (
    <ProfileTrigger userId="u1" name="Ada" render={<span />}>
      @Ada
    </ProfileTrigger>
  ))
  const trigger = view.getByRole('button', { name: 'Open profile of Ada' })
  fireEvent.pointerDown(trigger)
  expect(prefetched).toEqual(['u1'])
  // an element that is not a button gets the keys of one
  fireEvent.keyDown(trigger, { key: ' ' })
  expect(opened.map((call) => call.instant)).toEqual([true])
})

test('without a member id, and outside the shell, the content is not a button', () => {
  const { view } = setup(() => (
    <ProfileTrigger userId={null} name="Former member">
      Former member
    </ProfileTrigger>
  ))
  expect(view.queryByRole('button')).toBeNull()
  cleanup()
  const outside = render(
    <ProfileTrigger userId="u1" name="Ada">
      Ada
    </ProfileTrigger>,
  )
  expect(outside.queryByRole('button')).toBeNull()
  expect(outside.getByText('Ada')).toBeTruthy()
})
