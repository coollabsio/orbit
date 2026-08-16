import type { Doc, DocBlock } from '../../mock/types'

/** Direct children of a doc (null = roots), in store order. */
export function childrenOf(docs: Doc[], parentId: string | null): Doc[] {
  return docs.filter((d) => d.parentId === parentId)
}

/** Ancestor chain of a doc, root first, excluding the doc itself. */
export function ancestorsOf(docs: Doc[], docId: string | null): Doc[] {
  const byId = new Map(docs.map((d) => [d.id, d]))
  const chain: Doc[] = []
  let current = docId ? byId.get(docId) : undefined
  const seen = new Set<string>()
  while (current && current.parentId && !seen.has(current.parentId)) {
    seen.add(current.parentId)
    const parent = byId.get(current.parentId)
    if (!parent) break
    chain.unshift(parent)
    current = parent
  }
  return chain
}

export const BLOCK_TYPES: Array<{ type: DocBlock['type']; label: string }> = [
  { type: 'p', label: 'Text' },
  { type: 'h1', label: 'Heading 1' },
  { type: 'h2', label: 'Heading 2' },
  { type: 'h3', label: 'Heading 3' },
  { type: 'bullet', label: 'Bulleted list' },
  { type: 'numbered', label: 'Numbered list' },
  { type: 'todo', label: 'To-do' },
  { type: 'quote', label: 'Quote' },
  { type: 'code', label: 'Code' },
  { type: 'divider', label: 'Divider' },
]

/** 1-based position of a numbered block within its consecutive run. */
export function numberedIndex(blocks: DocBlock[], index: number): number {
  let n = 1
  for (let i = index - 1; i >= 0 && blocks[i].type === 'numbered'; i--) n += 1
  return n
}
