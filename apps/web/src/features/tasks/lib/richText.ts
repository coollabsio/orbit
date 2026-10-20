/**
 * Whether BlockNote blocks say nothing: no text other than whitespace, no mention, no uploaded image or file. The
 * server applies the same rule to comments (`has_content` in `apps/server/src/repositories/tasks.rs`).
 */
export function isEmptyRichText(blocks: readonly unknown[]): boolean {
  return !hasContent(blocks)
}

function hasContent(value: unknown): boolean {
  if (Array.isArray(value)) return value.some(hasContent)
  if (typeof value !== 'object' || value === null) return false
  const node = value as Record<string, unknown>
  const props = node.props as Record<string, unknown> | undefined
  if (node.type === 'text') return typeof node.text === 'string' && node.text.trim() !== ''
  if (node.type === 'mention') return true
  if ((node.type === 'image' || node.type === 'file') && typeof props?.url === 'string' && props.url !== '') return true
  return ['content', 'children', 'rows', 'cells'].some((key) => hasContent(node[key]))
}

/** An image or file block for an uploaded file. */
export function fileBlock(file: File, url: string) {
  return { type: file.type.startsWith('image/') ? 'image' : 'file', props: { url, name: file.name } }
}

/**
 * What Save does in a comment's edit: `close` without a change (an old markdown comment is not rewritten), `save` an
 * edit, and `confirm-delete` when the text was removed (a comment needs content). Removing the text of a comment that
 * has files keeps the comment as it was.
 */
export function commentSaveAction({ content, changed, hasAttachments }: { content: readonly unknown[]; changed: boolean; hasAttachments: boolean }): 'close' | 'save' | 'confirm-delete' {
  if (!changed) return 'close'
  if (!isEmptyRichText(content)) return 'save'
  return hasAttachments ? 'close' : 'confirm-delete'
}
