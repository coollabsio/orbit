// Entries and rules of the page body's "@" picker (see `MentionMenu`).
import type { Transaction } from '@tiptap/pm/state'
import type { DefaultReactSuggestionItem } from '@blocknote/react'
import { MentionAvatar } from './PageMentionChip'
import { filterPageMentionCandidates, PRIVATE_PAGE_HINT, type PageMentionMember, type PageMentionOptions } from './pageMentions'
import type { PageEditorInstance } from './schema'

/** Picker entries for `query`; a private page's entries sit under the "only you" hint. */
export function pageMentionItems(
  options: Pick<PageMentionOptions, 'candidates' | 'privatePage'>,
  query: string,
  insert: (member: PageMentionMember) => void,
): DefaultReactSuggestionItem[] {
  return filterPageMentionCandidates(options.candidates, query).map((member) => ({
    title: member.name,
    subtext: member.handle ?? member.email,
    icon: <MentionAvatar member={member} />,
    group: options.privatePage ? PRIVATE_PAGE_HINT : undefined,
    onItemClick: () => insert(member),
  }))
}

/** Inserts a mention chip (and a space after it) at the cursor. */
export function insertPageMention(editor: PageEditorInstance, member: PageMentionMember) {
  editor.insertInlineContent([{ type: 'mention', props: { userId: member.id, name: member.name } }, ' '])
}

/** "@" opens the picker only at a word start (not inside "a@b.c") and never in code blocks. */
export function mentionTriggerAllowed(tr: Transaction): boolean {
  const { $from } = tr.selection
  if ($from.parent.type.spec.code) return false
  const before = $from.parent.textBetween(Math.max(0, $from.parentOffset - 1), $from.parentOffset, undefined, '￼')
  return before === '' || /\s/.test(before)
}
