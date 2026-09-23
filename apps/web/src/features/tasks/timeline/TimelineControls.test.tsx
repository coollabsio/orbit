import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { TimelineControls } from './TimelineControls'
import { ZOOM_PRESETS } from './timelineLib'

test('zoom presets report their px/day and mark the active one', () => {
  const calls: number[] = []
  let today = 0
  const view = render(<TimelineControls pxPerDay={ZOOM_PRESETS.month} onZoomChange={(px) => calls.push(px)} onToday={() => { today++ }} />)
  expect(view.getByRole('button', { name: 'Month' }).getAttribute('aria-pressed')).toBe('true')
  fireEvent.click(view.getByRole('button', { name: 'Week' }))
  fireEvent.click(view.getByRole('button', { name: 'Today' }))
  expect(calls).toEqual([ZOOM_PRESETS.week])
  expect(today).toBe(1)
})
