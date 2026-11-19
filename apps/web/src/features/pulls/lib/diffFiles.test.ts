import { expect, test } from 'bun:test'
import type { GithubPullFile } from '@orbit/api/generated/types.gen'
import { makeDiffFilesLoader } from './diffFiles'

const file = (path: string, previous: string | null = null): GithubPullFile => ({ path, previous_path: previous, status: previous ? 'renamed' : 'modified', additions: 1, deletions: 1, patch: '', viewed: false })

test('the old side comes from the merge base, the new side from the head', async () => {
  const loads: string[] = []
  const loader = makeDiffFilesLoader({
    files: () => [file('src/b.ts', 'src/a.ts')],
    headSha: 'h1',
    mergeBase: async () => 'm0',
    load: async (path, sha) => { loads.push(`${path}@${sha}`); return `text of ${path}` },
  })
  const result = await loader({ name: 'src/b.ts', prevName: 'src/a.ts' })
  expect(loads.sort()).toEqual(['src/a.ts@m0', 'src/b.ts@h1'])
  expect(result).toEqual({ oldFile: { name: 'src/a.ts', contents: 'text of src/a.ts' }, newFile: { name: 'src/b.ts', contents: 'text of src/b.ts' } })
})

test('a file that cannot be loaded as text fails the expansion', async () => {
  const loader = makeDiffFilesLoader({ files: () => [file('logo.png')], headSha: 'h1', mergeBase: async () => 'm0', load: async () => null })
  await expect(loader({ name: 'logo.png' })).rejects.toThrow('This file cannot be shown in full.')
})
