import { describe, expect, test } from 'bun:test'
import { act, fireEvent, render, renderHook } from '@testing-library/react'
import * as Y from 'yjs'
import { ProfilePopoverContext, type ProfileAction } from '@/components/common/profilePopover'
import { TooltipProvider } from '@/components/ui/tooltip'
import { FakeProvider } from '@/test/fakeCollab'
import { collaboratorsOf, useCollaborators, type Collaborator } from './presence'
import { PresenceAvatars } from './PresenceAvatars'

const person = (id: string, name = id, color = '#2563eb', cursor: unknown = null) => ({ user: { id, name, color }, cursor })

describe('collaboratorsOf', () => {
  test('dedupes by user id, hides the local client and the own user, keeps first-seen order', () => {
    const states = new Map<number, Record<string, unknown>>([
      [1, person('me', 'Me')],
      [2, person('ada', 'Ada', '#e11d48')],
      [3, person('bob', 'Bob')],
      [4, person('ada', 'Ada (tab 2)', '#e11d48', { anchor: {}, head: {} })], // editing in another tab
      [5, person('me', 'Me (other tab)')],
      [6, { cursor: null }], // not stamped by the server yet
    ])
    expect(collaboratorsOf(states, 1, 'me')).toEqual([
      { id: 'ada', name: 'Ada', color: '#e11d48', editing: true },
      { id: 'bob', name: 'Bob', color: '#2563eb', editing: false },
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
    Array.from({ length: count }, (_, index) => ({ id: `u${index}`, name: `User ${index} Name`, color: '#2563eb', editing: index % 2 === 0 }))

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
    // Hover/focus shows the name and whether they edit or only view.
    fireEvent.focus(avatars[1].parentElement!)
    expect(await view.findByText('User 1 Name')).toBeTruthy()
    expect(view.getByText('Viewing')).toBeTruthy()
  })

  test('a click opens the profile; for someone editing it offers "Go to cursor"', () => {
    const opened: { userId: string; action?: ProfileAction }[] = []
    const control = { open: (userId: string, _anchor: HTMLElement, options: { action?: ProfileAction }) => void opened.push({ userId, action: options.action }), prefetch: () => {} }
    const revealed: string[] = []
    const view = render(
      <ProfilePopoverContext value={control}>
        <TooltipProvider>
          <PresenceAvatars people={people(2)} onReveal={(person) => revealed.push(person.id)} />
        </TooltipProvider>
      </ProfilePopoverContext>,
    )
    fireEvent.click(view.getByRole('button', { name: 'Open profile of User 0 Name' }))
    fireEvent.click(view.getByRole('button', { name: 'Open profile of User 1 Name' }))
    expect(opened.map((item) => [item.userId, item.action?.label])).toEqual([
      ['u0', 'Go to cursor'],
      ['u1', undefined], // only viewing: no caret to go to
    ])
    opened[0].action!.run()
    expect(revealed).toEqual(['u0'])
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
