import { en } from '@blocknote/core/locales'

/** Placeholders of Orbit's comment fields (BlockNote reads them from the page editor's dictionary). */
export const COMMENT_PLACEHOLDERS = {
  new_comment: 'Add a comment… Use @ to mention',
  comment_reply: 'Reply…',
  edit_comment: 'Edit comment…',
} as const

/** The dictionary of a comment editor that shows `placeholder` while empty. */
export function commentEditorDictionary(placeholder: string) {
  return { ...en, placeholders: { ...en.placeholders, emptyDocument: placeholder } }
}
