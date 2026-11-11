import { useQueryClient } from '@tanstack/react-query'
import { forwardRef, useMemo, useRef } from 'react'
import { PatchDiff, Virtualizer, type FileDiffOptions } from '@pierre/diffs/react'
import { getGithubPullContents, getGithubPullMergeBase } from '@orbit/api/generated/sdk.gen'
import type { GithubPullDetail, GithubPullFile, GithubReviewThread } from '@orbit/api/generated/types.gen'
import { toGitPatch, type PullRef } from '@orbit/api/github'
import { apiClient } from '@/api/client'
import { pullKeys } from '@/features/pulls/api'
import { threadAnnotations } from '@/features/pulls/lib/annotations'
import { makeDiffFilesLoader } from '@/features/pulls/lib/diffFiles'
import { ThreadView } from '@/features/pulls/components/ThreadView'
import type { DiffStyle } from '@/features/pulls/useDiffStyle'
import { useTheme } from '@/lib/themeContext'
import '@/features/pulls/pulls.css'

/** The id of a file's section, for scrolling to it. */
export const fileAnchor = (path: string) => `file-${encodeURIComponent(path)}`

/** All changed files in one scroll list. Pierre renders only what is on screen, so large pull requests stay fast. */
export const PullDiffList = forwardRef<HTMLDivElement, {
  workspaceId: string
  pullRef: PullRef
  pull: GithubPullDetail
  files: GithubPullFile[]
  threads: GithubReviewThread[]
  diffStyle: DiffStyle
  highlightedThread: string | null
}>(function PullDiffList({ workspaceId, pullRef, pull, files, threads, diffStyle, highlightedThread }, ref) {
  const { theme } = useTheme()
  const queryClient = useQueryClient()
  // Expanding hidden lines needs both whole files: loaded once, on the first expand. The loader reads the files
  // through a ref, so a viewed tick does not hand every diff new options.
  const filesRef = useRef(files)
  filesRef.current = files
  const loadDiffFiles = useMemo(() => {
    const pathParams = { workspace_id: workspaceId, owner: pullRef.owner, repo: pullRef.repo, number: pullRef.number }
    return makeDiffFilesLoader({
      files: () => filesRef.current,
      headSha: pull.head_sha,
      mergeBase: async () => (await queryClient.fetchQuery({
        queryKey: [...pullKeys.pull(workspaceId, pullRef), 'merge-base', pull.base_sha, pull.head_sha],
        staleTime: Infinity,
        queryFn: async () => (await getGithubPullMergeBase({ client: apiClient, path: pathParams, throwOnError: true })).data,
      })).sha,
      load: async (path, sha) => (await queryClient.fetchQuery({
        queryKey: pullKeys.contents(workspaceId, pullRef, path, sha),
        staleTime: Infinity,
        queryFn: async () => (await getGithubPullContents({ client: apiClient, path: pathParams, query: { path, ref: sha }, throwOnError: true })).data,
      })).text ?? null,
    })
  }, [pull.base_sha, pull.head_sha, pullRef, queryClient, workspaceId])

  const options = useMemo((): FileDiffOptions<GithubReviewThread, undefined> => ({
    diffStyle,
    themeType: theme,
    overflow: 'scroll',
    hunkSeparators: 'line-info',
    loadDiffFiles,
  }), [diffStyle, theme, loadDiffFiles])

  return <Virtualizer className="h-full overflow-y-auto" contentClassName="flex flex-col gap-4 p-4">
    <div ref={ref} className="contents">
      {files.map((file) => <FileSection key={file.path} file={file} pull={pull} threads={threads} options={options} highlightedThread={highlightedThread} />)}
    </div>
  </Virtualizer>
})

function FileSection({ file, pull, threads, options, highlightedThread }: {
  file: GithubPullFile
  pull: GithubPullDetail
  threads: GithubReviewThread[]
  options: FileDiffOptions<GithubReviewThread, undefined>
  highlightedThread: string | null
}) {
  const patch = useMemo(() => toGitPatch(file), [file])
  const annotations = useMemo(() => threadAnnotations(threads, file.path), [threads, file.path])
  const fileOptions = useMemo(() => ({ ...options, collapsed: file.viewed }), [options, file.viewed])
  return <section id={fileAnchor(file.path)} data-file-path={file.path} aria-label={file.path} className="scroll-mt-4 overflow-hidden rounded-lg border">
    {patch ? <PatchDiff<GithubReviewThread, undefined>
      patch={patch}
      options={fileOptions}
      lineAnnotations={annotations}
      renderAnnotation={(annotation) => annotation.metadata ? <ThreadView thread={annotation.metadata} highlighted={annotation.metadata.id === highlightedThread} /> : null}
    /> : <div className="flex flex-wrap items-center gap-2 px-3 py-2.5 text-sm">
      <span className="min-w-0 truncate font-mono text-[13px]">{file.path}</span>
      <span className="text-xs text-muted-foreground">+{file.additions} −{file.deletions}</span>
      <span className="ml-auto text-xs text-muted-foreground">
        {file.status === 'renamed' && file.additions + file.deletions === 0 ? 'Renamed, no changes.' : 'Too large to show here.'}{' '}
        <a className="text-primary underline" href={`${pull.html_url}/files`} target="_blank" rel="noopener noreferrer" aria-label={`Open ${file.path} on GitHub`}>Open on GitHub</a>
      </span>
    </div>}
  </section>
}
