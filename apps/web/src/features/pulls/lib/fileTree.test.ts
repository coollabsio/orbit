import { expect, test } from 'bun:test'
import type { GithubPullFile } from '@orbit/api/generated/types.gen'
import { buildFileTree, type TreeNode } from './fileTree'

const file = (path: string): GithubPullFile => ({ path, previous_path: null, status: 'modified', additions: 1, deletions: 0, patch: '', viewed: false })
const names = (nodes: TreeNode[]): unknown[] => nodes.map((node) => node.kind === 'folder' ? { [node.name]: names(node.children) } : node.name)

test('folders come first, then files, each sorted by name', () => {
  expect(names(buildFileTree([file('b.ts'), file('src/z.ts'), file('a.ts'), file('src/a.ts')]))).toEqual([{ src: ['a.ts', 'z.ts'] }, 'a.ts', 'b.ts'])
})

test('a folder with only one folder inside is shown as one path', () => {
  expect(names(buildFileTree([file('apps/web/src/a.ts'), file('apps/web/src/b.ts'), file('apps/web/c.ts')]))).toEqual([{ 'apps/web': [{ src: ['a.ts', 'b.ts'] }, 'c.ts'] }])
})

test('folder paths are full paths, for their keys', () => {
  const [folder] = buildFileTree([file('apps/web/a.ts')])
  expect(folder.kind === 'folder' ? folder.path : null).toBe('apps/web')
})
