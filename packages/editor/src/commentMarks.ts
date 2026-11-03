import { CommentsExtension, DefaultThreadStoreAuth, ThreadStore } from '@blocknote/core/comments'

/** A store without threads: the editor keeps the comment marks of the document but shows and creates no threads. */
class NoThreadStore extends ThreadStore {
  constructor() {
    super(new DefaultThreadStoreAuth('', 'comment'))
  }
  addThreadToDocument = undefined
  createThread = async (): Promise<never> => Promise.reject(new Error('Comments are not available here.'))
  addComment = async (): Promise<never> => Promise.reject(new Error('Comments are not available here.'))
  updateComment = async () => {}
  deleteComment = async () => {}
  deleteThread = async () => {}
  resolveThread = async () => {}
  unresolveThread = async () => {}
  addReaction = async () => {}
  deleteReaction = async () => {}
  getThread = (threadId: string): never => {
    throw new Error(`Thread ${threadId} not found`)
  }
  getThreads = () => new Map()
  subscribe = () => () => {}
}

/**
 * The `comment` mark for a collaborative page editor that shows no comments. Every editor bound to a page's Y.Doc
 * needs it: without the mark, y-prosemirror drops the text of anchored comments for everyone.
 */
export function commentMarks() {
  return CommentsExtension({ threadStore: new NoThreadStore(), resolveUsers: async () => [] })
}
