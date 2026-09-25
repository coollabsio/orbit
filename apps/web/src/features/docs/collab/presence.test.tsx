import { describe, expect, test } from 'bun:test'
import { act, fireEvent, render, renderHook } from '@testing-library/react'
import * as Y from 'yjs'
import { TooltipProvider } from '@/components/ui/tooltip'
import { FakeProvider } from '@/test/fakeCollab'
import { COLLAB_COLORS, collabColor } from './palette'
import { collaboratorsOf, useCollaborators, type Collaborator } from './presence'
import { PresenceAvatars } from './PresenceAvatars'

const person = (id: string, name = id, color = '#2563eb') => ({ user: { id, name, color } })

describe('collabColor', () => {
  test('mirrors the server hash (hub.rs user_color) over the 8-color palette', () => {
    // Values computed with the Rust algorithm: hash = hash * 31 + byte (wrapping u32), then % 8.
    expect(collabColor('01a0d778-64cb-7422-a4df-c7eae799677f')).toBe('#db2777')
    expect(collabColor('00000000-0000-7000-8000-000000000001')).toBe('#9333ea')
    expect(collabColor('')).toBe(COLLAB_COLORS[0])
    // Long ids wrap around 32 bits instead of losing precision.
    expect(COLLAB_COLORS).toContain(collabColor('x'.repeat(500)) as (typeof COLLAB_COLORS)[number])
  })
})

describe('collaboratorsOf', () => {
  test('dedupes by user id, hides the local client and the own user, keeps first-seen order', () => {
    const states = new Map<number, Record<string, unknown>>([
      [1, person('me', 'Me')],
      [2, person('ada', 'Ada', '#e11d48')],
      [3, person('bob', 'Bob')],
      [4, person('ada', 'Ada (tab 2)')],
      [5, person('me', 'Me (other tab)')],
      [6, { cursor: null }], // not stamped by the server yet
    ])
    expect(collaboratorsOf(states, 1, 'me')).toEqual([
      { id: 'ada', name: 'Ada', color: '#e11d48' },
      { id: 'bob', name: 'Bob', color: '#2563eb' },
    ])
    // Without a known user id only the local client is hidden.
    expect(collaboratorsOf(states, 1, null).map((item) => item.id)).toEqual(['ada', 'bob', 'me'])
  })

  test('useCollaborators follows awareness joins and leaves', () => {
    const provider = new FakeProvider(new Y.Doc())
    const { result } = renderHook(() => useCollaborators(provider.awareness, 'me'))
    expect(result.current).toEqual([])
    let ada = 0
    act(() => {
      ada = provider.addPeer({ id: 'ada', name: 'Ada', color: '#e11d48' })
      provider.addPeer({ id: 'me', name: 'Me', color: '#2563eb' })
    })
    expect(result.current.map((item) => item.name)).toEqual(['Ada'])
    act(() => {
      provider.awareness.states.delete(ada)
      provider.awareness.emit('change', [{ added: [], updated: [], removed: [ada] }, 'remote'])
    })
    expect(result.current).toEqual([])
    act(() => provider.destroy())
  })
})

describe('PresenceAvatars', () => {
  const people = (count: number): Collaborator[] =>
    Array.from({ length: count }, (_, index) => ({ id: `u${index}`, name: `User ${index} Name`, color: COLLAB_COLORS[index % 8] }))

  test('nothing while alone', () => {
    const view = render(<PresenceAvatars people={[]} />)
    expect(view.container.innerHTML).toBe('')
  })

  test('up to four avatars in the caret colors, then "+n" naming the rest', async () => {
    const view = render(
      <TooltipProvider>
        <PresenceAvatars people={people(6)} />
      </TooltipProvider>,
    )
    const group = view.getByRole('group')
    expect(group.getAttribute('aria-label')).toBe('Also here: User 0 Name, User 1 Name, User 2 Name, User 3 Name, User 4 Name, User 5 Name')
    const avatars = group.querySelectorAll('[data-presence-user]')
    expect(avatars).toHaveLength(4)
    expect(avatars[0].textContent).toBe('U0')
    expect((avatars[0].querySelector('[data-slot="avatar-fallback"]') as HTMLElement).style.backgroundColor).toBeTruthy()
    const more = view.getByText('+2')
    expect(more.getAttribute('aria-label')).toBe('2 more: User 4 Name, User 5 Name')
    // Hover/focus shows the names.
    fireEvent.focus(avatars[1])
    expect(await view.findByText('User 1 Name')).toBeTruthy()
  })

  test('exactly four people: no overflow counter', () => {
    const view = render(
      <TooltipProvider>
        <PresenceAvatars people={people(4)} />
      </TooltipProvider>,
    )
    expect(view.container.querySelectorAll('[data-presence-user]')).toHaveLength(4)
    expect(view.queryByText(/^\+/)).toBeNull()
  })
})
