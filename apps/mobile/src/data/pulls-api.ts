/** Pull requests, read from GitHub with the member's own account (the web's `features/pulls/api.ts`). No React. */
import {
  createGithubReviewComment,
  deleteGithubPullBranch,
  deleteGithubReviewComment,
  discardGithubReview,
  getGithubPull,
  mergeGithubPull,
  replyGithubReviewThread,
  resolveGithubReviewThread,
  submitGithubReview,
  unresolveGithubReviewThread,
  listGithubLinks,
  listGithubProjectPulls,
  listGithubPullConversation,
  listGithubPullFiles,
  listGithubPullThreads,
  markGithubFileViewed,
  unmarkGithubFileViewed,
} from '@orbit/api/generated/sdk.gen'
import type { GithubLink, GithubMergePull, GithubMergeResult, GithubPullDetail, GithubPullFile, GithubPullList, GithubPullThreads, GithubPullTimelineItem, GithubReviewComment, GithubReviewThread, GithubSubmitReview } from '@orbit/api/generated/types.gen'
import type { CommentTarget, PullRef } from '@orbit/api/github'
import { ApiProblem } from '@orbit/api/problem'

import type { OrbitClient } from './orbit'

export type PullFilter = 'open' | 'review' | 'mine' | 'merged' | 'closed'

export const PULL_FILTERS: { value: PullFilter; label: string; empty: string }[] = [
  { value: 'open', label: 'Open', empty: 'No open pull requests' },
  { value: 'review', label: 'Needs my review', empty: 'Nothing waits for your review' },
  { value: 'mine', label: 'Mine', empty: 'You have no open pull requests' },
  { value: 'merged', label: 'Merged', empty: 'No merged pull requests' },
  { value: 'closed', label: 'Closed', empty: 'No closed pull requests' },
]

export const pullKeys = {
  list: (workspaceId: string, projectId: string, filter: PullFilter) => ['github-pulls', workspaceId, 'list', projectId, filter] as const,
  pull: (workspaceId: string, ref: PullRef) => ['github-pulls', workspaceId, ref.owner.toLowerCase(), ref.repo.toLowerCase(), ref.number] as const,
  files: (workspaceId: string, ref: PullRef) => [...pullKeys.pull(workspaceId, ref), 'files'] as const,
  threads: (workspaceId: string, ref: PullRef) => [...pullKeys.pull(workspaceId, ref), 'threads'] as const,
  conversation: (workspaceId: string, ref: PullRef) => [...pullKeys.pull(workspaceId, ref), 'conversation'] as const,
  taskLinks: (workspaceId: string, taskId: string) => ['task-github-links', workspaceId, taskId] as const,
}

const path = (workspaceId: string, ref: PullRef) => ({ workspace_id: workspaceId, owner: ref.owner, repo: ref.repo, number: ref.number })

export async function pullList(client: OrbitClient, workspaceId: string, projectId: string, filter: PullFilter, cursor: string | null): Promise<GithubPullList> {
  return (await listGithubProjectPulls({ client, path: { workspace_id: workspaceId, project_id: projectId }, query: { filter, cursor: cursor ?? undefined }, throwOnError: true })).data
}

export async function pull(client: OrbitClient, workspaceId: string, ref: PullRef): Promise<GithubPullDetail> {
  return (await getGithubPull({ client, path: path(workspaceId, ref), throwOnError: true })).data
}

export async function pullFiles(client: OrbitClient, workspaceId: string, ref: PullRef): Promise<GithubPullFile[]> {
  return (await listGithubPullFiles({ client, path: path(workspaceId, ref), throwOnError: true })).data
}

export async function pullThreads(client: OrbitClient, workspaceId: string, ref: PullRef): Promise<GithubPullThreads> {
  return (await listGithubPullThreads({ client, path: path(workspaceId, ref), throwOnError: true })).data
}

export async function pullConversation(client: OrbitClient, workspaceId: string, ref: PullRef): Promise<GithubPullTimelineItem[]> {
  return (await listGithubPullConversation({ client, path: path(workspaceId, ref), throwOnError: true })).data
}

export async function setViewed(client: OrbitClient, workspaceId: string, ref: PullRef, file: string, viewed: boolean): Promise<void> {
  const options = { client, path: path(workspaceId, ref), body: { path: file }, throwOnError: true } as const
  await (viewed ? markGithubFileViewed(options) : unmarkGithubFileViewed(options))
}

export async function taskGithubLinks(client: OrbitClient, workspaceId: string, taskId: string): Promise<GithubLink[]> {
  return (await listGithubLinks({ client, path: { workspace_id: workspaceId, task_id: taskId }, throwOnError: true })).data
}

/** The 409 codes that mean "connect GitHub first", and the connection status to show for each. */
export function githubPromptStatus(error: unknown): 'disconnected' | 'broken' | 'unavailable' | 'needs_recreate' | null {
  if (!(error instanceof ApiProblem) || error.status !== 409) return null
  switch (error.code) {
    case 'github_not_connected':
      return 'disconnected'
    case 'github_reconnect':
      return 'broken'
    case 'github_app_missing':
      return 'unavailable'
    case 'github_app_recreate':
      return 'needs_recreate'
    default:
      return null
  }
}

export const isNotFound = (error: unknown) => error instanceof ApiProblem && error.status === 404

/** The diff screen of one file; `thread` is the thread to show (from a notification). */
export function fileHref(pull: Pick<GithubPullDetail, 'repository' | 'number'>, path: string, thread?: string): string {
  const [owner = '', repo = ''] = pull.repository.split('/')
  const query = `path=${encodeURIComponent(path)}${thread ? `&thread=${encodeURIComponent(thread)}` : ''}`
  return `/tasks/pulls/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/${pull.number}/file?${query}`
}

export async function addComment(client: OrbitClient, workspaceId: string, ref: PullRef, target: CommentTarget, body: string, mode: 'pending' | 'single'): Promise<GithubReviewThread> {
  return (await createGithubReviewComment({ client, path: path(workspaceId, ref), body: { ...target, start_side: target.start_side ?? null, start_line: target.start_line ?? null, body, mode }, throwOnError: true })).data
}

export async function reply(client: OrbitClient, workspaceId: string, ref: PullRef, threadId: string, body: string): Promise<GithubReviewComment> {
  return (await replyGithubReviewThread({ client, path: { ...path(workspaceId, ref), thread_id: threadId }, body: { body }, throwOnError: true })).data
}

export async function deleteComment(client: OrbitClient, workspaceId: string, ref: PullRef, commentId: string): Promise<void> {
  await deleteGithubReviewComment({ client, path: { ...path(workspaceId, ref), comment_id: commentId }, throwOnError: true })
}

export async function setResolved(client: OrbitClient, workspaceId: string, ref: PullRef, threadId: string, resolved: boolean): Promise<GithubReviewThread> {
  const options = { client, path: { ...path(workspaceId, ref), thread_id: threadId }, throwOnError: true } as const
  return (await (resolved ? resolveGithubReviewThread(options) : unresolveGithubReviewThread(options))).data
}

export async function submitReview(client: OrbitClient, workspaceId: string, ref: PullRef, review: GithubSubmitReview): Promise<void> {
  await submitGithubReview({ client, path: path(workspaceId, ref), body: review, throwOnError: true })
}

export async function discardReview(client: OrbitClient, workspaceId: string, ref: PullRef): Promise<void> {
  await discardGithubReview({ client, path: path(workspaceId, ref), throwOnError: true })
}

export async function merge(client: OrbitClient, workspaceId: string, ref: PullRef, body: GithubMergePull): Promise<GithubMergeResult> {
  return (await mergeGithubPull({ client, path: path(workspaceId, ref), body, throwOnError: true })).data
}

export async function deleteBranch(client: OrbitClient, workspaceId: string, ref: PullRef): Promise<void> {
  await deleteGithubPullBranch({ client, path: path(workspaceId, ref), throwOnError: true })
}

/** GitHub's own words when it refuses, or `fallback`. */
export function githubMessage(error: unknown, fallback: string): string {
  return error instanceof ApiProblem && error.problem.detail ? error.problem.detail : fallback
}
