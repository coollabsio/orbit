// BlockNote's shadcn comment editor plus an "@" suggestion menu for mentions (installed through the components
// context, see `PageEditor`). Composer, replies and edits all use it; read-only comments render mentions too.
import { use, forwardRef } from 'react'
import {
  FormattingToolbar,
  FormattingToolbarController,
  getFormattingToolbarItems,
  SuggestionMenuController,
  useBlockNoteContext,
  type ComponentProps,
} from '@blocknote/react'
import { BlockNoteView } from '@blocknote/shadcn'
import { cn } from 'cn'
import { insertMention, mentionItems, type CommentEditorInstance } from './mentions'
import { MentionCandidatesContext } from './mentionContext'

function CommentFormattingToolbar() {
  const items = getFormattingToolbarItems([]).filter((item) => item.key !== 'nestBlockButton' && item.key !== 'unnestBlockButton')
  return <FormattingToolbar blockTypeSelectItems={[]}>{items}</FormattingToolbar>
}

export const CommentEditor = forwardRef<HTMLDivElement, ComponentProps['Comments']['Editor']>(function CommentEditor(props, ref) {
  const { className, onFocus, onBlur, autoFocus, editor, editable } = props
  const context = useBlockNoteContext()
  const candidates = use(MentionCandidatesContext)
  const commentEditor = editor as unknown as CommentEditorInstance
  return (
    <BlockNoteView
      autoFocus={autoFocus}
      className={cn(className, 'orbit-comment-editor')}
      theme={context?.colorSchemePreference}
      editor={editor}
      sideMenu={false}
      slashMenu={false}
      tableHandles={false}
      filePanel={false}
      formattingToolbar={false}
      editable={editable}
      ref={ref}
      onFocus={onFocus}
      onBlur={onBlur}
    >
      <FormattingToolbarController formattingToolbar={CommentFormattingToolbar} />
      {editable ? (
        <SuggestionMenuController
          triggerCharacter="@"
          getItems={async (query) => mentionItems(candidates, query, (candidate) => insertMention(commentEditor, candidate))}
        />
      ) : null}
    </BlockNoteView>
  )
})
