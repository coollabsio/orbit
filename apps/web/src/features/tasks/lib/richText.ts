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
