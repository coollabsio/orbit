import { expect, mock, test } from 'bun:test'
import { DESCRIPTION_AUTOSAVE_MS, createAutosave } from './descriptionAutosave'

const doc = (text: string) => ({
  type: 'doc' as const,
  content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
})

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

test('the debounce is 800ms and only the last edit is saved', async () => {
  expect(DESCRIPTION_AUTOSAVE_MS).toBe(800)
  const save = mock((_document: unknown) => {})
  const autosave = createAutosave(save, 20)

  autosave.change(doc('a'))
  autosave.change(doc('ab'))
  autosave.change(doc('abc'))
  expect(save).not.toHaveBeenCalled()

  await wait(40)
  expect(save).toHaveBeenCalledTimes(1)
  expect(save).toHaveBeenCalledWith(doc('abc'))
})

test('flush saves immediately and cancels the pending timer', async () => {
  const save = mock((_document: unknown) => {})
  const autosave = createAutosave(save, 20)

  autosave.change(doc('typed'))
  autosave.flush(doc('typed'))
  expect(save).toHaveBeenCalledTimes(1)

  await wait(40)
  expect(save).toHaveBeenCalledTimes(1)
})

test('flush with an unchanged document does not save again', async () => {
  const save = mock((_document: unknown) => {})
  const autosave = createAutosave(save, 20)

  autosave.change(doc('typed'))
  await wait(40)
  autosave.flush(doc('typed'))

  expect(save).toHaveBeenCalledTimes(1)
})

test('cancel drops a pending save entirely', async () => {
  const save = mock((_document: unknown) => {})
  const autosave = createAutosave(save, 20)

  autosave.change(doc('dropped'))
  autosave.cancel()

  await wait(40)
  expect(save).not.toHaveBeenCalled()
})

test('the document the editor opened with is never saved back unchanged', () => {
  const save = mock((_document: unknown) => {})
  const autosave = createAutosave(save, 20, doc('stored'))

  autosave.flush(doc('stored'))

  expect(save).not.toHaveBeenCalled()
})

test('a save waits for the one in flight and then sends only the newest document', async () => {
  const finish: Array<() => void> = []
  const save = mock((_document: unknown) => new Promise<void>((resolve) => finish.push(resolve)))
  const autosave = createAutosave(save, 20)

  autosave.flush(doc('one'))
  autosave.flush(doc('two'))
  autosave.flush(doc('three'))
  expect(save).toHaveBeenCalledTimes(1)

  finish[0]()
  await wait(0)
  expect(save).toHaveBeenCalledTimes(2)
  expect(save).toHaveBeenLastCalledWith(doc('three'))
})

test('a failed save can be retried with the same document', async () => {
  const save = mock((_document: unknown) => Promise.reject(new Error('409')))
  const autosave = createAutosave(save, 20)

  autosave.flush(doc('typed'))
  await wait(0)
  autosave.flush(doc('typed'))

  expect(save).toHaveBeenCalledTimes(2)
})
