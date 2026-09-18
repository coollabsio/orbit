import { Extension, type AnyExtension, type JSONContent } from '@tiptap/core'
import { TaskItem } from '@tiptap/extension-list/task-item'
import { TaskList } from '@tiptap/extension-list/task-list'
import { Placeholder } from '@tiptap/extensions/placeholder'
import StarterKit from '@tiptap/starter-kit'
import type { SuggestionOptions } from '@tiptap/suggestion'
import { IdentifierPaste } from './identifierPaste'
import { createMentionSuggestion, type MentionContext } from './mentionSuggestion'
import { PersonMention } from './personMention'
import { TaskMention } from './taskMention'
import type { TaskResolver } from './taskResolver'

export interface EditorKeyHandlers {
  /** Cmd/Ctrl+Enter with the editor's JSON. Returns false when there is nothing to submit, so the key falls through to a hard break. */
  submit: (document: JSONContent) => boolean
  /** Escape outside the mention menu. */
  cancel: () => boolean
}

// Rejects any explicit scheme other than http(s); scheme-less text (`example.com`)
// still autolinks, with the default `http` protocol.
const OTHER_SCHEME = /^(?!https?:)[a-z][a-z0-9+.-]*:/i

/**
 * Priority 101, listed after the mention node: the suggestion plugin (also 101)
 * sees Enter and Escape first, and these keys beat StarterKit's hard break
 * (100), which also binds Mod-Enter.
 */
function editorKeys(keys: EditorKeyHandlers) {
  return Extension.create({
    name: 'orbitEditorKeys',
    priority: 101,
    addKeyboardShortcuts() {
      return {
        'Mod-Enter': () => keys.submit(this.editor.getJSON()),
        Escape: () => keys.cancel(),
      }
    },
  })
}

/**
 * Every extension an Orbit editor loads. Only allowlisted nodes and marks
 * exist in this schema, so pasted images, tables and the like are dropped at
 * the door; `toServerDocument` then strips the attributes TipTap adds on top.
 */
export function buildEditorExtensions({
  placeholder,
  mentions,
  resolve,
  keys,
  taskMention = TaskMention,
}: {
  placeholder: string
  mentions: MentionContext
  resolve: TaskResolver | null
  keys: EditorKeyHandlers
  /** The surface swaps in a node view that shows the live title; tests use the bare node. */
  taskMention?: AnyExtension
}): AnyExtension[] {
  return [
    // StarterKit v3 already supplies Link and Underline — do not add them again.
    StarterKit.configure({
      heading: { levels: [1, 2, 3] },
      link: {
        openOnClick: false,
        HTMLAttributes: { target: '_blank', rel: 'noopener noreferrer', class: null },
        isAllowedUri: (url, context) => context.defaultValidate(url) && !OTHER_SCHEME.test(url),
      },
    }),
    Placeholder.configure({ placeholder }),
    TaskList,
    TaskItem.configure({ nested: true }),
    taskMention,
    IdentifierPaste.configure({ resolve }),
    // A SINGLE unified "@": people and issues in one grouped menu.
    // Mention types the picked value as its own attrs; ours is a MentionItem that
    // our `command` turns into either node, so the option type is widened here.
    PersonMention.configure({
      suggestion: createMentionSuggestion(mentions) as unknown as Omit<SuggestionOptions, 'editor'>,
    }),
    editorKeys(keys),
  ]
}
