import type { Attribute } from '@tiptap/core'
import Mention from '@tiptap/extension-mention'

/**
 * TipTap's Mention node with exactly the allowlisted attributes: `id` and
 * `label`. The stock node also stores `mentionSuggestionChar`, which the server
 * rejects with a 422. Backspace deletes the whole mention, because the stock
 * behaviour (turning it back into its trigger character) reads that attribute.
 */
export const PersonMention = Mention.extend({
  addOptions() {
    const parent = this.parent?.()
    return {
      ...parent!,
      deleteTriggerWithBackspace: true,
      HTMLAttributes: { class: 'editor-mention' },
      renderText: ({ node }) => `@${String(node.attrs.label ?? '')}`,
      renderHTML: ({ options, node }) => ['span', options.HTMLAttributes, `@${String(node.attrs.label ?? '')}`],
    }
  },

  addAttributes() {
    const { id, label } = (this.parent?.() ?? {}) as Record<string, Attribute>
    return { id, label }
  },
})
