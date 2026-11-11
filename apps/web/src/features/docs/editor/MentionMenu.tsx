// The page body's "@" picker: members who can see the page, filtered by the query, keyboard navigable (BlockNote's
// suggestion menu). Picking one inserts a `mention` chip and a space. Its "#" picker finds tasks (`TaskMenu`).
import { useCallback, useState } from 'react'
import { SuggestionMenuController } from '@blocknote/react'
import { useTaskReferenceSearch } from '@/features/tasks/taskReferences'
import { insertPageMention, mentionTriggerAllowed, pageMentionItems } from './mentionItems'
import type { PageMentionOptions } from './pageMentions'
import { insertTaskLink, latestQuery, TASK_TRIGGER, taskMenuItems, taskTriggerAllowed } from './taskMentionItems'
import type { PageEditorInstance } from '@orbit/editor/schema'

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

/** "#": tasks by title or ID (recently updated ones first), inserted as a link "ENG-12 Title". */
export function TaskMenu({ editor }: { editor: Parameters<typeof insertTaskLink>[0] }) {
  const search = useTaskReferenceSearch()
  const [latest] = useState(latestQuery)
  // stable: BlockNote loads the items again whenever `getItems` changes, and the editor renders often (presence)
  const getItems = useCallback(
    async (query: string) => {
      latest.set(query)
      return taskMenuItems(query, search, (task) => insertTaskLink(editor, task), latest.is(query))
    },
    [latest, search, editor],
  )
  if (!search) return null
  return <SuggestionMenuController triggerCharacter={TASK_TRIGGER} shouldOpen={taskTriggerAllowed} getItems={getItems} />
}
