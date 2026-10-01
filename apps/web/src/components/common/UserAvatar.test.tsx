import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { UserAvatar } from './UserAvatar'

const person = { name: 'Ada Lovelace', color: '#ff0000', online: true }

const presenceOf = (ui: React.ReactElement) => {
  const view = render(ui)
  const labels = [...view.container.querySelectorAll('[data-slot=avatar-badge]')].map((badge) => badge.textContent)
  view.unmount()
  return labels
}

test('presence shows only when asked for, with a text label for each state', () => {
  expect(presenceOf(<UserAvatar user={person} />)).toEqual([])
  expect(presenceOf(<UserAvatar user={person} showOnline />)).toEqual(['Online'])
  expect(presenceOf(<UserAvatar user={{ ...person, online: false }} showOnline />)).toEqual(['Offline'])
})

test('the online prop wins over the person, and a person without presence gets it from the prop', () => {
  expect(presenceOf(<UserAvatar user={person} online={false} showOnline />)).toEqual(['Offline'])
  expect(presenceOf(<UserAvatar user={{ name: 'Ada Lovelace', color: '#ff0000' }} online showOnline />)).toEqual(['Online'])
  // nothing is known about this person, so nothing is claimed
  expect(presenceOf(<UserAvatar user={{ name: 'Ada Lovelace', color: '#ff0000' }} showOnline />)).toEqual([])
})
