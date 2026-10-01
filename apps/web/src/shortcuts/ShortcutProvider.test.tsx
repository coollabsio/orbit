import { afterEach, expect, spyOn, test } from 'bun:test'
import { act, render, renderHook } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import type { Overrides } from './bindings'
import type { CommandId } from './commands'
import { ShortcutProvider } from './ShortcutProvider'
import { useActiveCommands, useAvailableCommands, useCommand, useRunCommand } from './useCommand'

function Probe({ id, run, enabled }: { id: CommandId; run: (() => void) | null; enabled?: boolean }) {
  useCommand(id, run, { enabled })
  return null
}

const calls: string[] = []
const record = (name: string) => () => void calls.push(name)
afterEach(() => {
  calls.length = 0
  document.body.innerHTML = ''
})

const mount = (children: ReactNode, overrides?: Overrides) => render(<ShortcutProvider overrides={overrides}>{children}</ShortcutProvider>)

test('a single key runs its command, but not while typing in an input', async () => {
  const user = userEvent.setup()
  mount(<><Probe id="task.setStatus" run={record('status')} /><input aria-label="field" /></>)
  await user.keyboard('s')
  expect(calls).toEqual(['status'])
  await user.click(document.querySelector('input')!)
  await user.keyboard('s')
  expect(calls).toEqual(['status'])
})

test('the command menu key works while typing; other Mod keys keep their text meaning', async () => {
  const user = userEvent.setup()
  mount(<><Probe id="palette.open" run={record('palette')} /><Probe id="list.selectAll" run={record('all')} /><input aria-label="field" /></>)
  await user.click(document.querySelector('input')!)
  await user.keyboard('{Control>}k{/Control}')
  await user.keyboard('{Control>}a{/Control}')
  expect(calls).toEqual(['palette'])
})

test('an open dialog blocks page commands but not the command menu', async () => {
  const user = userEvent.setup()
  mount(<><Probe id="task.setStatus" run={record('status')} /><Probe id="palette.open" run={record('palette')} /><div role="dialog" /></>)
  await user.keyboard('s')
  await user.keyboard('{Control>}k{/Control}')
  expect(calls).toEqual(['palette'])
})

test('a sequence runs its command and not the single-key command of its last step', async () => {
  const user = userEvent.setup()
  mount(<><Probe id="nav.inbox" run={record('inbox')} /><Probe id="task.assignMe" run={record('assign')} /></>)
  await user.keyboard('gi')
  expect(calls).toEqual(['inbox'])
  await user.keyboard('i')
  expect(calls).toEqual(['inbox', 'assign'])
})

test('a sequence does not run when the second key comes too late', async () => {
  const user = userEvent.setup()
  mount(<Probe id="nav.inbox" run={record('inbox')} />)
  await user.keyboard('g')
  await act(() => new Promise((resolve) => setTimeout(resolve, 1100)))
  await user.keyboard('i')
  expect(calls).toEqual([])
})

test('a command without a mounted handler is inactive', async () => {
  const user = userEvent.setup()
  const wrapper = ({ children }: { children: ReactNode }) => (
    <ShortcutProvider><Probe id="task.setStatus" run={null} /><Probe id="task.setPriority" run={record('priority')} enabled={false} /><Probe id="task.create" run={record('create')} />{children}</ShortcutProvider>
  )
  const { result } = renderHook(() => useActiveCommands(), { wrapper })
  await user.keyboard('sp')
  expect(calls).toEqual([])
  expect(result.current.map((command) => command.id)).toEqual(['task.create'])
})

test('useRunCommand runs the mounted handler and ignores an inactive command', () => {
  const wrapper = ({ children }: { children: ReactNode }) => <ShortcutProvider><Probe id="task.create" run={record('create')} />{children}</ShortcutProvider>
  const { result } = renderHook(() => useRunCommand(), { wrapper })
  act(() => result.current('task.create'))
  act(() => result.current('task.setStatus'))
  expect(calls).toEqual(['create'])
})

test('a key the editor already handled does not run a command', async () => {
  const user = userEvent.setup()
  mount(<><Probe id="view.layout" run={record('layout')} /><div data-testid="editor" tabIndex={0} onKeyDown={(event) => event.preventDefault()} /></>)
  await user.click(document.querySelector('[data-testid=editor]')!)
  await user.keyboard('{Control>}b{/Control}')
  expect(calls).toEqual([])
})

test('a held key repeats focus movement only', () => {
  mount(<><Probe id="list.down" run={record('down')} /><Probe id="task.create" run={record('create')} /></>)
  const press = (code: string, key: string) => act(() => void document.body.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, repeat: true, code, key })))
  press('KeyJ', 'j')
  press('KeyC', 'c')
  expect(calls).toEqual(['down'])
})

test('a handler that throws does not stop other shortcuts', async () => {
  const user = userEvent.setup()
  const error = spyOn(console, 'error').mockImplementation(() => {})
  mount(<><Probe id="task.setStatus" run={() => { throw new Error('boom') }} /><Probe id="task.setPriority" run={record('priority')} /></>)
  await user.keyboard('sp')
  expect(calls).toEqual(['priority'])
  error.mockRestore()
})

test('an override replaces the default key', async () => {
  const user = userEvent.setup()
  mount(<Probe id="task.setStatus" run={record('status')} />, { 'task.setStatus': 'Q' })
  await user.keyboard('sq')
  expect(calls).toEqual(['status'])
})

test('the last mounted handler wins and unmounting restores the previous one', async () => {
  const user = userEvent.setup()
  const view = mount(<><Probe id="task.create" run={record('outer')} /><Probe id="task.create" run={record('inner')} /></>)
  await user.keyboard('c')
  view.rerender(<ShortcutProvider><Probe id="task.create" run={record('outer')} /></ShortcutProvider>)
  await user.keyboard('c')
  expect(calls).toEqual(['inner', 'outer'])
})

test('a held Mod key still keeps the browser from acting, without running the command again', () => {
  mount(<Probe id="view.save" run={record('save')} />)
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, repeat: true, code: 'KeyS', key: 's', ctrlKey: true })
  act(() => void document.body.dispatchEvent(event))
  expect(calls).toEqual([])
  expect(event.defaultPrevented).toBe(true)
})

test('a command that is not available right now leaves the key alone', async () => {
  let available = false
  function Target() {
    useCommand('task.setStatus', record('status'), { available: () => available })
    return null
  }
  const wrapper = ({ children }: { children: ReactNode }) => <ShortcutProvider><Target />{children}</ShortcutProvider>
  const { result } = renderHook(() => useAvailableCommands(), { wrapper })
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, code: 'KeyS', key: 's' })
  act(() => void document.body.dispatchEvent(event))
  expect(calls).toEqual([])
  expect(event.defaultPrevented).toBe(false)
  expect(result.current()).toEqual([])
  available = true
  await userEvent.keyboard('s')
  expect(calls).toEqual(['status'])
  expect(result.current().map((command) => command.id)).toEqual(['task.setStatus'])
})

test('a held or repeated first key still makes one sequence, never a single-key command too', () => {
  mount(<><Probe id="nav.inbox" run={record('inbox')} /><Probe id="task.assignMe" run={record('assign')} /></>)
  const press = (code: string, key: string, repeat = false) => act(() => void document.body.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, repeat, code, key })))
  press('KeyG', 'g')
  press('KeyG', 'g', true)
  press('KeyI', 'i')
  expect(calls).toEqual(['inbox'])
  press('KeyG', 'g')
  press('KeyG', 'g')
  press('KeyI', 'i')
  expect(calls).toEqual(['inbox', 'inbox'])
})

test('a Mod key after the first key of a sequence still runs its command', async () => {
  const user = userEvent.setup()
  mount(<><Probe id="nav.inbox" run={record('inbox')} /><Probe id="palette.open" run={record('palette')} /></>)
  await user.keyboard('g{Control>}k{/Control}')
  expect(calls).toEqual(['palette'])
})

test('a list on the page blocks shortcuts only while the focus is in it', async () => {
  const user = userEvent.setup()
  mount(<><Probe id="task.setStatus" run={record('status')} /><div role="listbox" tabIndex={0} data-testid="list" /></>)
  await user.keyboard('s')
  expect(calls).toEqual(['status'])
  await user.click(document.querySelector('[data-testid=list]')!)
  await user.keyboard('s')
  expect(calls).toEqual(['status'])
})
