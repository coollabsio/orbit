import { isEmptyDocument } from '../../../components/editor/document'

export type CommentUploadMode = 'text' | 'attachment-only' | 'invalid'

/** An attachment-only comment is one whose document is empty. */
export function commentUploadMode(body: unknown, fileCount: number): CommentUploadMode {
  if (!isEmptyDocument(body)) return 'text'
  return fileCount > 0 ? 'attachment-only' : 'invalid'
}
