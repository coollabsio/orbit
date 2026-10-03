import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { UserAvatar } from './UserAvatar'

const person = { name: 'Ada Lovelace', color: '#ff0000' }

const presenceOf = (ui: React.ReactElement) => {
  const view = render(ui)
  const labels = [...view.container.querySelectorAll('[data-slot=avatar-badge]')].map((badge) => badge.textContent)
  view.unmount()
  return labels
}

test('the badge shows only with a status, and each status has a text label', () => {
  expect(presenceOf(<UserAvatar user={person} />)).toEqual([])
  expect(presenceOf(<UserAvatar user={person} status="online" />)).toEqual(['Online'])
  expect(presenceOf(<UserAvatar user={person} status="idle" />)).toEqual(['Idle'])
  expect(presenceOf(<UserAvatar user={person} status="dnd" />)).toEqual(['Do not disturb'])
  expect(presenceOf(<UserAvatar user={person} status="offline" />)).toEqual(['Offline'])
  expect(presenceOf(<UserAvatar user={person} status="invisible" />)).toEqual(['Invisible'])
})
