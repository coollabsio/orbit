/** Pull request queries and writes, over `data/pulls-api.ts`. */
import type { GithubMergePull, GithubPullFile, GithubSubmitReview } from '@orbit/api/generated/types.gen'
import type { CommentTarget, PullRef } from '@orbit/api/github'
import { ApiProblem } from '@orbit/api/problem'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import * as api from '../data/pulls-api'
import { useOrbit } from '../orbit-context'

/** No retry for "no access" and "connect GitHub first": a retry does not fix them. */
const retry = (count: number, error: unknown) => !(error instanceof ApiProblem && [403, 404, 409].includes(error.status)) && count < 2
const shared = { staleTime: 30_000, retry }

export function usePullList(projectId: string, filter: api.PullFilter) {
  const { client, workspaceId } = useOrbit()
  return useInfiniteQuery({
    ...shared,
    queryKey: api.pullKeys.list(workspaceId, projectId, filter),
    queryFn: ({ pageParam }) => api.pullList(client, workspaceId, projectId, filter, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor,
  })
}

export function usePull(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ ...shared, queryKey: api.pullKeys.pull(workspaceId, ref), queryFn: () => api.pull(client, workspaceId, ref) })
}

export function usePullFiles(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ ...shared, queryKey: api.pullKeys.files(workspaceId, ref), queryFn: () => api.pullFiles(client, workspaceId, ref) })
}

export function usePullThreads(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ ...shared, queryKey: api.pullKeys.threads(workspaceId, ref), queryFn: () => api.pullThreads(client, workspaceId, ref) })
}

export function usePullConversation(ref: PullRef, enabled: boolean) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ ...shared, enabled, queryKey: api.pullKeys.conversation(workspaceId, ref), queryFn: () => api.pullConversation(client, workspaceId, ref) })
}

/** Marks a file viewed on GitHub; the tick changes at once and goes back when GitHub refuses. */
export function useSetViewed(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const key = api.pullKeys.files(workspaceId, ref)
  return useMutation({
    mutationFn: ({ file, viewed }: { file: string; viewed: boolean }) => api.setViewed(client, workspaceId, ref, file, viewed),
    onMutate: async ({ file, viewed }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const before = queryClient.getQueryData<GithubPullFile[]>(key)
      queryClient.setQueryData<GithubPullFile[]>(key, (files) => files?.map((item) => (item.path === file ? { ...item, viewed } : item)))
      return { before }
    },
    onError: (_error, _variables, context) => {
      if (context?.before) queryClient.setQueryData(key, context.before)
    },
  })
}

export function useTaskGithubLinks(taskId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: api.pullKeys.taskLinks(workspaceId, taskId), queryFn: () => api.taskGithubLinks(client, workspaceId, taskId) })
}

/** Writes that change the threads (and the pending count): they refresh them from GitHub. */
function useThreadWrite<V, R>(ref: PullRef, write: (variables: V) => Promise<R>) {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({ mutationFn: write, onSuccess: () => void queryClient.invalidateQueries({ queryKey: api.pullKeys.threads(workspaceId, ref) }) })
}

export function useAddComment(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  return useThreadWrite(ref, ({ target, body, mode }: { target: CommentTarget; body: string; mode: 'pending' | 'single' }) => api.addComment(client, workspaceId, ref, target, body, mode))
}

export function useReply(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  return useThreadWrite(ref, ({ threadId, body }: { threadId: string; body: string }) => api.reply(client, workspaceId, ref, threadId, body))
}

export function useDeleteComment(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  return useThreadWrite(ref, (commentId: string) => api.deleteComment(client, workspaceId, ref, commentId))
}

export function useResolveThread(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  return useThreadWrite(ref, ({ threadId, resolved }: { threadId: string; resolved: boolean }) => api.setResolved(client, workspaceId, ref, threadId, resolved))
}

export function useSubmitReview(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (review: GithubSubmitReview) => api.submitReview(client, workspaceId, ref, review),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: api.pullKeys.pull(workspaceId, ref) }),
  })
}

export function useDiscardReview(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  return useThreadWrite(ref, () => api.discardReview(client, workspaceId, ref))
}

export function useMerge(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: GithubMergePull) => api.merge(client, workspaceId, ref, body),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: api.pullKeys.pull(workspaceId, ref) }),
  })
}

export function useDeleteBranch(ref: PullRef) {
  const { client, workspaceId } = useOrbit()
  return useMutation({ mutationFn: () => api.deleteBranch(client, workspaceId, ref) })
}
