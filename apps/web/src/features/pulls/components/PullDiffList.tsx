import { useQueryClient } from '@tanstack/react-query'
import { useCallback, useMemo, useRef, useState } from 'react'
import { PatchDiff, Virtualizer, type DiffLineAnnotation, type FileDiffOptions } from '@pierre/diffs/react'
import { getGithubPullContents, getGithubPullMergeBase } from '@orbit/api/generated/sdk.gen'
import type { GithubPullDetail, GithubPullFile, GithubReviewThread } from '@orbit/api/generated/types.gen'
import { asideReason, draftKey, toCommentTarget, toGitPatch, type AsideReason, type CommentTarget, type PullRef } from '@orbit/api/github'
import { apiClient } from '@/api/client'
import { pullKeys } from '@/features/pulls/api'
import { threadAnnotations } from '@/features/pulls/lib/annotations'
import { makeDiffFilesLoader } from '@/features/pulls/lib/diffFiles'
import { CommentComposer } from '@/features/pulls/components/CommentComposer'
import { ThreadView } from '@/features/pulls/components/ThreadView'
import type { DiffStyle } from '@/features/pulls/useDiffStyle'
import { useTheme } from '@/lib/themeContext'
import { useCommand } from '@/shortcuts/useCommand'
import '@/features/pulls/pulls.css'

/** The id of a file's section, for scrolling to it. */
export const fileAnchor = (path: string) => `file-${encodeURIComponent(path)}`

/** What a line of a diff carries: a review thread, or the comment box the member opened there. */
type Annotation = { thread: GithubReviewThread; composer?: undefined } | { thread?: undefined; composer: CommentTarget }

const REASON: Record<AsideReason, string> = { lockfile: 'Lockfile', generated: 'Generated file', snapshot: 'Test snapshot' }

/** All changed files in one scroll list: the files to review, then the files set aside, each folded to one row until
 *  it is opened. Pierre renders only what is on screen, so large pull requests stay fast. */
export function PullDiffList({ workspaceId, pullRef, pull, headSha, review, aside, opened, onOpen, threads, diffStyle, highlightedThread }: {
  workspaceId: string
  pullRef: PullRef
  pull: GithubPullDetail
  /** The head commit of the files on screen. After a push it stays the old one until Refresh: comments and the
   *  whole files for expanded lines must match the diff the reader sees, not the newest commit. */
  headSha: string
  review: GithubPullFile[]
  aside: GithubPullFile[]
  /** Files opened by hand: set-aside files, and viewed files that stay unfolded. */
  opened: ReadonlySet<string>
  onOpen: (path: string) => void
  threads: GithubReviewThread[]
  diffStyle: DiffStyle
  highlightedThread: string | null
}) {
  const files = useMemo(() => [...review, ...aside], [review, aside])
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
      headSha,
      mergeBase: async () => (await queryClient.fetchQuery({
        queryKey: [...pullKeys.pull(workspaceId, pullRef), 'merge-base', pull.base_sha, headSha],
        staleTime: Infinity,
        queryFn: async () => (await getGithubPullMergeBase({ client: apiClient, path: pathParams, throwOnError: true })).data,
      })).sha,
      load: async (path, sha) => (await queryClient.fetchQuery({
        queryKey: pullKeys.contents(workspaceId, pullRef, path, sha),
        staleTime: Infinity,
        queryFn: async () => (await getGithubPullContents({ client: apiClient, path: pathParams, query: { path, ref: sha }, throwOnError: true })).data,
      })).text ?? null,
    })
  }, [pull.base_sha, headSha, pullRef, queryClient, workspaceId])

  const canComment = pull.viewer.can_comment
  // one comment box at a time; the line under the pointer is where M opens it
  const [composer, setComposer] = useState<CommentTarget | null>(null)
  const hovered = useRef<CommentTarget | null>(null)
  const actions = useMemo(() => ({ workspaceId, pullRef }), [workspaceId, pullRef])
  useCommand('pull.comment', () => { if (hovered.current) setComposer(hovered.current) }, { enabled: canComment })
  const closeComposer = useCallback(() => setComposer(null), [])
  const setHovered = useCallback((target: CommentTarget) => { hovered.current = target }, [])

  const options = useMemo((): Omit<FileDiffOptions<Annotation, undefined>, 'onGutterUtilityClick' | 'onLineEnter'> => ({
    diffStyle,
    themeType: theme,
    overflow: 'scroll',
    hunkSeparators: 'line-info',
    loadDiffFiles,
    enableGutterUtility: canComment,
  }), [diffStyle, theme, loadDiffFiles, canComment])

  const section = (file: GithubPullFile) => <FileSection
        key={file.path}
        file={file}
        unfolded={opened.has(file.path)}
        pull={pull}
        threads={threads}
        options={options}
        highlightedThread={highlightedThread}
        composer={composer?.path === file.path ? composer : null}
        onComment={canComment ? setComposer : null}
        onHover={setHovered}
        renderComposer={(target) => <CommentComposer key={draftKey(workspaceId, pullRef, target)} workspaceId={workspaceId} pullRef={pullRef} headSha={headSha} target={target} onDone={closeComposer} />}
        actions={canComment ? actions : undefined}
      />
  return <Virtualizer className="h-full overflow-y-auto" contentClassName="flex flex-col gap-3 p-3 sm:p-4">
    {review.map(section)}
    {review.length === 0 ? <p className="py-6 text-center text-sm text-muted-foreground">Only machine-made files changed. They are below.</p> : null}
    {aside.length > 0 ? <div role="separator" aria-label="Files set aside" className="mt-3 flex items-center gap-3 text-xs text-muted-foreground">
      <span className="shrink-0">Set aside: {aside.length} {aside.length === 1 ? 'file' : 'files'} a person rarely reads</span>
      <span className="h-px flex-1 bg-border" />
    </div> : null}
    {aside.map((file) => opened.has(file.path) ? section(file) : <button
      key={file.path}
      type="button"
      id={fileAnchor(file.path)}
      onClick={() => onOpen(file.path)}
      className="flex scroll-mt-4 items-center gap-3 rounded-lg border bg-background px-3 py-2 text-left text-[13px] transition-colors duration-150 hover:bg-muted/60"
    >
      <span className="min-w-0 truncate font-mono text-xs">{file.path}</span>
      <span className="shrink-0 text-xs text-muted-foreground">{REASON[asideReason(file.path) ?? 'generated']}</span>
      <span className="ml-auto shrink-0 font-mono text-xs tabular-nums"><span className="text-emerald-600 dark:text-emerald-400">+{file.additions}</span> <span className="text-red-600 dark:text-red-400">−{file.deletions}</span></span>
      <span className="shrink-0 text-xs text-primary">Show diff</span>
    </button>)}
  </Virtualizer>
}

function FileSection({ file, unfolded, pull, threads, options, highlightedThread, composer, onComment, onHover, renderComposer, actions }: {
  file: GithubPullFile
  unfolded: boolean
  pull: GithubPullDetail
  threads: GithubReviewThread[]
  options: Omit<FileDiffOptions<Annotation, undefined>, 'onGutterUtilityClick' | 'onLineEnter'>
  highlightedThread: string | null
  composer: CommentTarget | null
  onComment: ((target: CommentTarget) => void) | null
  onHover: (target: CommentTarget) => void
  renderComposer: (target: CommentTarget) => React.ReactNode
  actions?: { workspaceId: string; pullRef: PullRef }
}) {
  const patch = useMemo(() => toGitPatch(file), [file])
  const annotations = useMemo(() => {
    const list: DiffLineAnnotation<Annotation>[] = threadAnnotations(threads, file.path).map((item) => ({ side: item.side, lineNumber: item.lineNumber, metadata: { thread: item.metadata as GithubReviewThread } }))
    if (composer) list.push({ side: composer.side === 'LEFT' ? 'deletions' : 'additions', lineNumber: composer.line, metadata: { composer } })
    return list
  }, [threads, file.path, composer])
  const path = file.path
  const fileOptions = useMemo((): FileDiffOptions<Annotation, undefined> => ({
    ...options,
    collapsed: file.viewed && !unfolded,
    onGutterUtilityClick: onComment ? (range) => onComment(toCommentTarget(path, range)) : undefined,
    onLineEnter: (props) => onHover({ path, side: props.annotationSide === 'deletions' ? 'LEFT' : 'RIGHT', line: props.lineNumber }),
  }), [options, file.viewed, unfolded, onComment, onHover, path])
  return <section id={fileAnchor(file.path)} data-file-path={file.path} aria-label={file.path} className="scroll-mt-4 overflow-hidden rounded-lg border bg-background shadow-xs">
    {patch ? <PatchDiff<Annotation, undefined>
      patch={patch}
      options={fileOptions}
      lineAnnotations={annotations}
      renderAnnotation={(annotation) => annotation.metadata?.composer
        ? renderComposer(annotation.metadata.composer)
        : annotation.metadata?.thread ? <ThreadView key={annotation.metadata.thread.id} thread={annotation.metadata.thread} highlighted={annotation.metadata.thread.id === highlightedThread} actions={actions} /> : null}
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
