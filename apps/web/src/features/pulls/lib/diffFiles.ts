import type { GithubPullFile } from '@orbit/api/generated/types.gen'

type Loaded = { name: string; contents: string }

/**
 * Pierre's `loadDiffFiles`: both whole files of one diff, to show hidden lines. The old side is the merge base (what
 * GitHub diffs against: the base branch may have moved on), the new side the head commit. `files` is read on each
 * call, so the loader itself never changes (a new loader would re-render every diff).
 */
export function makeDiffFilesLoader({ files, headSha, mergeBase, load }: {
  files: () => GithubPullFile[]
  headSha: string
  mergeBase: () => Promise<string>
  load: (path: string, sha: string) => Promise<string | null>
}) {
  return async (diff: { name: string; prevName?: string }): Promise<{ oldFile: Loaded; newFile: Loaded }> => {
    const file = files().find((item) => item.path === diff.name)
    const newPath = file?.path ?? diff.name
    const oldPath = file?.previous_path ?? diff.prevName ?? newPath
    const read = async (path: string, sha: string): Promise<Loaded> => {
      const contents = await load(path, sha)
      if (contents == null) throw new Error('This file cannot be shown in full.')
      return { name: path, contents }
    }
    const [oldFile, newFile] = await Promise.all([mergeBase().then((sha) => read(oldPath, sha)), read(newPath, headSha)])
    return { oldFile, newFile }
  }
}
