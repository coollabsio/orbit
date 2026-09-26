import { createElement } from 'react'
// @mentions inside comments: a `mention` inline node ({ userId, name }) in the comment editor's schema, picked from
// the members who can see the page after typing "@". The server reads mentions from the body and notifies them.
import { BlockNoteSchema, createParagraphBlockSpec, defaultInlineContentSpecs, defaultStyleSpecs } from '@blocknote/core'
import { createReactInlineContentSpec, type DefaultReactSuggestionItem } from '@blocknote/react'
import type { MentionCandidate } from './mentionable'
import { MentionChip } from './MentionChip'

export type { MentionCandidate } from './mentionable'
export { MentionCandidatesContext, MentionNamesContext } from './mentionContext'

export const mentionSpec = createReactInlineContentSpec(
  {
    type: 'mention',
    propSchema: {
      userId: { default: '' },
      name: { default: '' },
    },
    content: 'none',
  },
  {
    render: ({ inlineContent }) => createElement(MentionChip, { userId: inlineContent.props.userId, name: inlineContent.props.name }),
  },
)

// Like BlockNote's default comment schema (paragraphs, no colors), plus mentions.
const { textColor: _textColor, backgroundColor: _backgroundColor, ...styleSpecs } = defaultStyleSpecs

export const commentEditorSchema = BlockNoteSchema.create({
  blockSpecs: { paragraph: createParagraphBlockSpec() },
  inlineContentSpecs: { ...defaultInlineContentSpecs, mention: mentionSpec },
  styleSpecs,
})

export type CommentEditorInstance = typeof commentEditorSchema.BlockNoteEditor

/** Picker entries for `query` (name or handle, case-insensitive), at most 8. */
export function mentionItems(
  candidates: readonly MentionCandidate[],
  query: string,
  insert: (candidate: MentionCandidate) => void,
): DefaultReactSuggestionItem[] {
  const needle = query.trim().toLowerCase()
  return candidates
    .filter((candidate) => !needle || candidate.name.toLowerCase().includes(needle) || candidate.handle?.toLowerCase().includes(needle))
    .slice(0, 8)
    .map((candidate) => ({
      title: candidate.name,
      subtext: candidate.handle,
      onItemClick: () => insert(candidate),
    }))
}

/** Inserts a mention (and a space after it) at the comment editor's cursor. */
export function insertMention(editor: CommentEditorInstance, candidate: MentionCandidate) {
  editor.insertInlineContent([{ type: 'mention', props: { userId: candidate.id, name: candidate.name } }, ' '])
}
