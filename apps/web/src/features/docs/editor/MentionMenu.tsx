// The page body's "@" picker: members who can see the page, filtered by the query, keyboard navigable (BlockNote's
// suggestion menu). Picking one inserts a `mention` chip and a space.
import { SuggestionMenuController } from '@blocknote/react'
import { insertPageMention, mentionTriggerAllowed, pageMentionItems } from './mentionItems'
import type { PageMentionOptions } from './pageMentions'
import type { PageEditorInstance } from './schema'

export function PageMentionMenu({ editor, options }: { editor: PageEditorInstance; options: () => PageMentionOptions | undefined }) {
  return (
    <SuggestionMenuController
      triggerCharacter="@"
      shouldOpen={mentionTriggerAllowed}
      getItems={async (query) => {
        const current = options()
        if (!current) return []
        return pageMentionItems(current, query, (member) => insertPageMention(editor, member))
      }}
    />
  )
}
