import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createGithubReviewComment, deleteGithubPullBranch, deleteGithubReviewComment, discardGithubReview, getGithubPull, getGithubPullContents, listGithubProjectPulls, listGithubPullConversation, listGithubPullFiles, listGithubPullThreads, markGithubFileViewed, mergeGithubPull, replyGithubReviewThread, resolveGithubReviewThread, submitGithubReview, unmarkGithubFileViewed, unresolveGithubReviewThread, updateGithubReviewComment } from '@orbit/api/generated/sdk.gen'
import type { GithubMergePull, GithubPullFile, GithubPullThreads, GithubReviewComment, GithubReviewThread, GithubSubmitReview } from '@orbit/api/generated/types.gen'
import type { CommentTarget, PullRef } from '@orbit/api/github'
import { ApiProblem } from '@orbit/api/problem'
import { apiClient } from '@/api/client'

export type PullFilter = 'open' | 'review' | 'mine' | 'merged' | 'closed'

/**
 * Pull request data comes from GitHub with each member's own token. It lives outside `queryKeys.workspace(…)` on
 * purpose: a workspace change refetches that whole prefix, and these queries go to GitHub.
 */
export const pullKeys = {
  all: (workspaceId: string) => ['github-pulls', workspaceId] as const,
  list: (workspaceId: string, projectId: string, filter: PullFilter, search: string) => ['github-pulls', workspaceId, 'list', projectId, filter, search] as const,
  pull: (workspaceId: string, ref: PullRef) => ['github-pulls', workspaceId, ref.owner.toLowerCase(), ref.repo.toLowerCase(), ref.number] as const,
  /** The files of one head commit: a push never swaps the files under the reader (see `PullPage`). */
  files: (workspaceId: string, ref: PullRef, head: string) => [...pullKeys.pull(workspaceId, ref), 'files', head] as const,
  threads: (workspaceId: string, ref: PullRef) => [...pullKeys.pull(workspaceId, ref), 'threads'] as const,
  conversation: (workspaceId: string, ref: PullRef) => [...pullKeys.pull(workspaceId, ref), 'conversation'] as const,
  contents: (workspaceId: string, ref: PullRef, path: string, sha: string) => [...pullKeys.pull(workspaceId, ref), 'contents', path, sha] as const,
}

/** GitHub errors that a retry does not fix: no access, or the member must connect GitHub first. */
function retryable(count: number, error: unknown) {
  if (error instanceof ApiProblem && (error.status === 404 || error.status === 409 || error.status === 403)) return false
  return count < 2
}

const shared = { staleTime: 30_000, retry: retryable }
const path = (workspaceId: string, ref: PullRef) => ({ workspace_id: workspaceId, owner: ref.owner, repo: ref.repo, number: ref.number })

export function usePullList(workspaceId: string, projectId: string, filter: PullFilter, search: string, cursor: string | null) {
  return useQuery({
    ...shared,
    queryKey: [...pullKeys.list(workspaceId, projectId, filter, search), cursor],
    queryFn: async () => (await listGithubProjectPulls({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, query: { filter, q: search || undefined, cursor: cursor ?? undefined }, throwOnError: true })).data,
    placeholderData: keepPreviousData,
  })
}

export function usePull(workspaceId: string, ref: PullRef) {
  return useQuery({ ...shared, queryKey: pullKeys.pull(workspaceId, ref), queryFn: async () => (await getGithubPull({ client: apiClient, path: path(workspaceId, ref), throwOnError: true })).data })
}

export function usePullFiles(workspaceId: string, ref: PullRef, head: string, enabled = true) {
  return useQuery({
    ...shared,
    enabled,
    // the diff changes only when the reader asks for it (Refresh), never on focus or reconnect
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    queryKey: pullKeys.files(workspaceId, ref, head),
    queryFn: async () => (await listGithubPullFiles({ client: apiClient, path: path(workspaceId, ref), throwOnError: true })).data,
  })
}

export function usePullThreads(workspaceId: string, ref: PullRef, enabled = true) {
  return useQuery({ ...shared, enabled, queryKey: pullKeys.threads(workspaceId, ref), queryFn: async () => (await listGithubPullThreads({ client: apiClient, path: path(workspaceId, ref), throwOnError: true })).data })
}

export function usePullConversation(workspaceId: string, ref: PullRef, enabled = true) {
  return useQuery({ ...shared, enabled, queryKey: pullKeys.conversation(workspaceId, ref), queryFn: async () => (await listGithubPullConversation({ client: apiClient, path: path(workspaceId, ref), throwOnError: true })).data })
}

/** A file at a commit, loaded when the reader expands hidden lines. `text` is `null` for binary or large files. */
export function useFileContents(workspaceId: string, ref: PullRef, file: string, sha: string, enabled: boolean) {
  return useQuery({
    ...shared,
    enabled,
    staleTime: Infinity,
    queryKey: pullKeys.contents(workspaceId, ref, file, sha),
    queryFn: async () => (await getGithubPullContents({ client: apiClient, path: path(workspaceId, ref), query: { path: file, ref: sha }, throwOnError: true })).data,
  })
}

/** Marks a file viewed on GitHub; the tick changes at once and goes back when GitHub refuses. */
export function useSetViewed(workspaceId: string, ref: PullRef, head: string) {
  const queryClient = useQueryClient()
  const key = pullKeys.files(workspaceId, ref, head)
  return useMutation({
    mutationFn: async ({ file, viewed }: { file: string; viewed: boolean }) => {
      const options = { client: apiClient, path: path(workspaceId, ref), body: { path: file }, throwOnError: true } as const
      await (viewed ? markGithubFileViewed(options) : unmarkGithubFileViewed(options))
    },
    onMutate: async ({ file, viewed }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const before = queryClient.getQueryData<GithubPullFile[]>(key)
      queryClient.setQueryData<GithubPullFile[]>(key, (files) => files?.map((item) => item.path === file ? { ...item, viewed } : item))
      return { before }
    },
    onError: (_error, _variables, context) => { if (context?.before) queryClient.setQueryData(key, context.before) },
  })
}

/** The 409 codes that mean "connect GitHub first" (the prompt explains each). */
export function needsGithubConnection(error: unknown): boolean {
  return error instanceof ApiProblem && error.status === 409 && ['github_not_connected', 'github_reconnect', 'github_app_missing', 'github_app_recreate'].includes(error.code)
}

/** The thread list with `thread` put in place (new threads go last). */
function withThread(data: GithubPullThreads | undefined, thread: GithubReviewThread): GithubPullThreads | undefined {
  if (!data) return data
  const known = data.threads.some((item) => item.id === thread.id)
  return { ...data, threads: known ? data.threads.map((item) => item.id === thread.id ? thread : item) : [...data.threads, thread] }
}

function useThreadsCache(workspaceId: string, ref: PullRef) {
  const queryClient = useQueryClient()
  const key = pullKeys.threads(workspaceId, ref)
  return {
    put: (thread: GithubReviewThread) => queryClient.setQueryData<GithubPullThreads>(key, (data) => withThread(data, thread)),
    // the pending count and other threads may have changed on GitHub
    refresh: () => void queryClient.invalidateQueries({ queryKey: key }),
  }
}

/** A comment on a line or a range: `pending` goes into the member's review, `single` is posted now. */
export function useAddComment(workspaceId: string, ref: PullRef) {
  const threads = useThreadsCache(workspaceId, ref)
  return useMutation({
    mutationFn: async ({ target, body, mode, headSha }: { target: CommentTarget; body: string; mode: 'pending' | 'single'; headSha: string }) =>
      (await createGithubReviewComment({ client: apiClient, path: path(workspaceId, ref), body: { ...target, start_side: target.start_side ?? null, start_line: target.start_line ?? null, body, mode, head_sha: headSha }, throwOnError: true })).data,
    onSuccess: (thread) => { threads.put(thread); threads.refresh() },
  })
}

export function useReply(workspaceId: string, ref: PullRef) {
  const threads = useThreadsCache(workspaceId, ref)
  return useMutation({
    mutationFn: async ({ threadId, body }: { threadId: string; body: string }) =>
      (await replyGithubReviewThread({ client: apiClient, path: { ...path(workspaceId, ref), thread_id: threadId }, body: { body }, throwOnError: true })).data,
    onSuccess: () => threads.refresh(),
  })
}

export function useEditComment(workspaceId: string, ref: PullRef) {
  const threads = useThreadsCache(workspaceId, ref)
  return useMutation({
    mutationFn: async ({ commentId, body }: { commentId: string; body: string }): Promise<GithubReviewComment> =>
      (await updateGithubReviewComment({ client: apiClient, path: { ...path(workspaceId, ref), comment_id: commentId }, body: { body }, throwOnError: true })).data,
    onSuccess: () => threads.refresh(),
  })
}

export function useDeleteComment(workspaceId: string, ref: PullRef) {
  const threads = useThreadsCache(workspaceId, ref)
  return useMutation({
    mutationFn: async (commentId: string) => { await deleteGithubReviewComment({ client: apiClient, path: { ...path(workspaceId, ref), comment_id: commentId }, throwOnError: true }) },
    onSuccess: () => threads.refresh(),
  })
}

export function useResolveThread(workspaceId: string, ref: PullRef) {
  const threads = useThreadsCache(workspaceId, ref)
  return useMutation({
    mutationFn: async ({ threadId, resolved }: { threadId: string; resolved: boolean }) => {
      const options = { client: apiClient, path: { ...path(workspaceId, ref), thread_id: threadId }, throwOnError: true } as const
      return (await (resolved ? resolveGithubReviewThread(options) : unresolveGithubReviewThread(options))).data
    },
    onSuccess: (thread) => threads.put(thread),
  })
}

/** Submits the member's review (their pending comments go with it). */
export function useSubmitReview(workspaceId: string, ref: PullRef) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (review: GithubSubmitReview) => { await submitGithubReview({ client: apiClient, path: path(workspaceId, ref), body: review, throwOnError: true }) },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: pullKeys.pull(workspaceId, ref) }),
  })
}

export function useDiscardReview(workspaceId: string, ref: PullRef) {
  const threads = useThreadsCache(workspaceId, ref)
  return useMutation({
    mutationFn: async () => { await discardGithubReview({ client: apiClient, path: path(workspaceId, ref), throwOnError: true }) },
    onSuccess: () => threads.refresh(),
  })
}

/** After new commits: the pull request, its files and threads again. */
export function useRefreshPull(workspaceId: string, ref: PullRef) {
  const queryClient = useQueryClient()
  return () => void queryClient.invalidateQueries({ queryKey: pullKeys.pull(workspaceId, ref) })
}

export function useMerge(workspaceId: string, ref: PullRef) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (merge: GithubMergePull) => (await mergeGithubPull({ client: apiClient, path: path(workspaceId, ref), body: merge, throwOnError: true })).data,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: pullKeys.pull(workspaceId, ref) }),
  })
}

export function useDeleteBranch(workspaceId: string, ref: PullRef) {
  return useMutation({
    mutationFn: async () => { await deleteGithubPullBranch({ client: apiClient, path: path(workspaceId, ref), throwOnError: true }) },
  })
}

/** GitHub's own words when it refuses, or `fallback`. */
export function githubMessage(error: unknown, fallback: string): string {
  return error instanceof ApiProblem && error.problem.detail ? error.problem.detail : fallback
}
