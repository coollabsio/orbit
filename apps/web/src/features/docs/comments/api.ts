import { useQuery } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import {
  createPageComment,
  createPageThread,
  deletePageComment,
  deletePageThread,
  listPageThreads,
  reopenPageThread,
  resolvePageThread,
  updatePageComment,
} from '@/api/generated/sdk.gen'
import type { PageComment, PageThread } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

type ApiClient = ReturnType<typeof createApiClient>

/** A comment body: BlockNote blocks of the comment editor (paragraphs with text, links and mentions). */
export type CommentBlocks = unknown[]

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

/**
 * The page's threads (open and resolved). Under the workspace prefix, so every realtime event refreshes it: comment
 * changes are audited and reach other viewers that way.
 */
export function threadsQueryOptions(workspaceId: string, pageId: string, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.pages.threads(workspaceId, pageId),
    queryFn: async (): Promise<PageThread[]> => {
      const { data } = await listPageThreads({ client, path: { workspace_id: workspaceId, page_id: pageId }, throwOnError: true })
      return required(data, 'Threads response was empty.').items
    },
  }
}

export function usePageThreads(workspaceId: string, pageId: string) {
  return useQuery(threadsQueryOptions(workspaceId, pageId))
}

/** Threads still open (the header badge) and resolved ones (the panel's tabs). */
export function threadCounts(threads: readonly PageThread[] | undefined): { open: number; resolved: number } {
  let open = 0
  let resolved = 0
  for (const thread of threads ?? []) {
    if (!thread.comments.some((comment) => !comment.deleted_at)) continue
    if (thread.resolved) resolved += 1
    else open += 1
  }
  return { open, resolved }
}

export interface CommentsApi {
  createThread: (body: CommentBlocks, quote: string) => Promise<PageThread>
  addComment: (threadId: string, body: CommentBlocks) => Promise<PageComment>
  updateComment: (threadId: string, commentId: string, body: CommentBlocks) => Promise<PageComment>
  deleteComment: (threadId: string, commentId: string) => Promise<void>
  deleteThread: (threadId: string) => Promise<void>
  resolveThread: (threadId: string) => Promise<PageThread>
  reopenThread: (threadId: string) => Promise<PageThread>
}

/** REST calls for one page's comments. */
export function commentsApi(workspaceId: string, pageId: string, client: ApiClient = apiClient): CommentsApi {
  const page = { workspace_id: workspaceId, page_id: pageId }
  return {
    createThread: async (body, quote) => {
      const { data } = await createPageThread({ client, path: page, body: { body: body as never, quote }, throwOnError: true })
      return required(data, 'Thread response was empty.')
    },
    addComment: async (threadId, body) => {
      const { data } = await createPageComment({ client, path: { ...page, thread_id: threadId }, body: { body: body as never }, throwOnError: true })
      return required(data, 'Comment response was empty.')
    },
    updateComment: async (threadId, commentId, body) => {
      const { data } = await updatePageComment({
        client,
        path: { ...page, thread_id: threadId, comment_id: commentId },
        body: { body: body as never },
        throwOnError: true,
      })
      return required(data, 'Comment response was empty.')
    },
    deleteComment: async (threadId, commentId) => {
      await deletePageComment({ client, path: { ...page, thread_id: threadId, comment_id: commentId }, throwOnError: true })
    },
    deleteThread: async (threadId) => {
      await deletePageThread({ client, path: { ...page, thread_id: threadId }, throwOnError: true })
    },
    resolveThread: async (threadId) => {
      const { data } = await resolvePageThread({ client, path: { ...page, thread_id: threadId }, throwOnError: true })
      return required(data, 'Thread response was empty.')
    },
    reopenThread: async (threadId) => {
      const { data } = await reopenPageThread({ client, path: { ...page, thread_id: threadId }, throwOnError: true })
      return required(data, 'Thread response was empty.')
    },
  }
}
