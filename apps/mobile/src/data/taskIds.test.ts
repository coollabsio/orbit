import { expect, test } from 'bun:test'

import { paragraphBlocks } from './docs'
import { findTaskIds, linkTaskIdsInMarkdown } from './taskIds'

const keys = ['ENG']

test('a task ID is a whole word of a project key; code, links and other words stay text', () => {
  expect(findTaskIds('see ENG-12, eng-3 and UTF-8', keys).map((id) => id.identifier)).toEqual(['ENG-12', 'ENG-3'])
  expect(findTaskIds('`ENG-1` [ENG-2](https://x.test) https://x.test/ENG-3 abc-ENG-4 #ENG-5', keys)).toEqual([])
})

test('a task comment sends each typed ID as a markdown link to the task', () => {
  expect(linkTaskIdsInMarkdown('fixes ENG-12.', keys, 'https://orbit.test/')).toBe('fixes [ENG-12](https://orbit.test/tasks/ENG-12).')
})

test('a page comment sends each typed ID as a link node', () => {
  const [block] = paragraphBlocks('see ENG-12 now', () => 'b1', [], { keys, server: 'https://orbit.test' })
  expect(block!.content).toEqual([
    { type: 'text', text: 'see ', styles: {} },
    { type: 'link', href: 'https://orbit.test/tasks/ENG-12', content: [{ type: 'text', text: 'ENG-12', styles: {} }] },
    { type: 'text', text: ' now', styles: {} },
  ])
})
