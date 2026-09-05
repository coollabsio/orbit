export type CommentUploadMode = 'text' | 'attachment-only' | 'invalid'

export function commentUploadMode(body: string, fileCount: number): CommentUploadMode {
  if (body.trim()) return 'text'
  return fileCount > 0 ? 'attachment-only' : 'invalid'
}
