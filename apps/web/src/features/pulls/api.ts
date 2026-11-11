import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { getGithubPull, getGithubPullContents, listGithubProjectPulls, listGithubPullConversation, listGithubPullFiles, listGithubPullThreads, markGithubFileViewed, unmarkGithubFileViewed } from '@orbit/api/generated/sdk.gen'
import type { GithubPullFile } from '@orbit/api/generated/types.gen'
import type { PullRef } from '@orbit/api/github'
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
  files: (workspaceId: string, ref: PullRef) => [...pullKeys.pull(workspaceId, ref), 'files'] as const,
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

export function usePullFiles(workspaceId: string, ref: PullRef, enabled = true) {
  return useQuery({ ...shared, enabled, queryKey: pullKeys.files(workspaceId, ref), queryFn: async () => (await listGithubPullFiles({ client: apiClient, path: path(workspaceId, ref), throwOnError: true })).data })
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
export function useSetViewed(workspaceId: string, ref: PullRef) {
  const queryClient = useQueryClient()
  const key = pullKeys.files(workspaceId, ref)
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
