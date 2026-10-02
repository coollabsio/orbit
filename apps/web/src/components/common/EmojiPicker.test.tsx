import { afterEach, beforeEach, expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { emojibaseResponse } from '@/test/emojibase'
import { EmojiPicker } from './EmojiPicker'

const realFetch = globalThis.fetch

beforeEach(() => {
  globalThis.fetch = (async (input: RequestInfo | URL) => emojibaseResponse(input) ?? new Response(null, { status: 404 })) as typeof fetch
})

afterEach(() => {
  globalThis.fetch = realFetch
})

test('a click on an emoji gives its character', async () => {
  const picked: string[] = []
  const view = render(<EmojiPicker onPick={(emoji) => picked.push(emoji)} />)
  await userEvent.click(await view.findByRole('gridcell', { name: 'Grinning face' }, { timeout: 4000 }))
  expect(picked).toEqual(['😀'])
})
