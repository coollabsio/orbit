import { Node, mergeAttributes } from '@tiptap/core'

/**
 * An ID-based issue reference. `id` is the truth; `identifier` is a cached
 * fallback rendered only when the target cannot be resolved. Atom + inline so a
 * chip is a single indivisible caret stop, exactly like the person mention.
 *
 * The static renderer does not run node views, so the live title comes from
 * `nodeMapping.tsx` / `TaskChip` on the read path — this node only owns the
 * document shape and the ProseMirror HTML representation.
 */
export const TaskMention = Node.create({
  name: 'taskMention',
  group: 'inline',
  inline: true,
  atom: true,
  selectable: true,
  draggable: false,

  addAttributes() {
    return {
      id: {
        default: null,
        parseHTML: (element: HTMLElement) => element.getAttribute('data-id'),
        renderHTML: (attributes: Record<string, unknown>) =>
          attributes.id ? { 'data-id': attributes.id } : {},
      },
      identifier: {
        default: null,
        parseHTML: (element: HTMLElement) => element.getAttribute('data-identifier'),
        renderHTML: (attributes: Record<string, unknown>) =>
          attributes.identifier ? { 'data-identifier': attributes.identifier } : {},
      },
    }
  },

  parseHTML() {
    return [{ tag: 'span[data-task-mention]' }]
  },

  renderHTML({ node, HTMLAttributes }) {
    return [
      'span',
      mergeAttributes({ 'data-task-mention': '', class: 'editor-chip' }, HTMLAttributes),
      String(node.attrs.identifier ?? ''),
    ]
  },

  renderText({ node }) {
    return String(node.attrs.identifier ?? '')
  },
})
