// The schema facts the server's BlockNote JSON <-> Yjs converter reads (`apps/server/src/collab/blocknote-schema.json`).
// Every host's editor schema must produce the checked-in table, or co-editing alters documents.
import { BlockNoteEditor, type BlockNoteSchema } from '@blocknote/core'

type Json = Record<string, unknown>

/** Schema facts the Rust converter needs: PM attr order and defaults per block, table cell attrs, style kinds. */
export function schemaTable(schema: BlockNoteSchema<any, any, any>) {
  const instance = BlockNoteEditor.create({ schema, _tiptapOptions: { injectCSS: false } })
  const pm = instance.pmSchema
  const blocks: Record<string, unknown> = {}
  const blockSchema = instance.schema.blockSchema as Record<string, { content: string; propSchema: Json }>
  for (const type of Object.keys(blockSchema).sort()) {
    const spec = blockSchema[type]
    const node = pm.nodes[type]
    if (!node) continue
    const created = node.create()
    const attrs = Object.keys(node.spec.attrs ?? {})
    blocks[type] = {
      content: node.spec.code ? 'plain' : spec.content,
      attrs: attrs.map((name) => ({
        name,
        prop: name in spec.propSchema,
        missing: created.attrs[name] === undefined ? { undefined: true } : { value: created.attrs[name] },
      })),
    }
  }
  const cell = pm.nodes.tableCell.create()
  const styles: Record<string, string> = {}
  const styleSchema = instance.schema.styleSchema as Record<string, { propSchema: string }>
  for (const name of Object.keys(styleSchema).sort()) styles[name] = styleSchema[name].propSchema
  // Custom inline content (mentions): PM attr order and defaults; `text` and `link` are built in.
  const inline: Record<string, unknown> = {}
  const inlineSchema = instance.schema.inlineContentSchema as Record<string, string | { content: string; propSchema: Json }>
  for (const type of Object.keys(inlineSchema).sort()) {
    const spec = inlineSchema[type]
    const node = pm.nodes[type]
    if (typeof spec === 'string' || !node) continue
    const created = node.create()
    inline[type] = {
      content: spec.content,
      attrs: Object.keys(node.spec.attrs ?? {}).map((name) => ({
        name,
        prop: name in spec.propSchema,
        missing: created.attrs[name] === undefined ? { undefined: true } : { value: created.attrs[name] },
      })),
    }
  }
  return {
    blocks,
    inline,
    cell: { attrs: Object.keys(pm.nodes.tableCell.spec.attrs ?? {}), defaults: cell.attrs },
    styles,
    container_attrs: Object.keys(pm.nodes.blockContainer.spec.attrs ?? {}),
  }
}
