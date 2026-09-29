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
import { cn } from 'cn'
import { ThemedBlockNoteView } from '../editor/ThemedBlockNoteView'
import { insertMention, mentionItems, type CommentEditorInstance } from './mentions'
import { MentionCandidatesContext } from './mentionContext'

function CommentFormattingToolbar() {
  const items = getFormattingToolbarItems([]).filter((item) => item.key !== 'nestBlockButton' && item.key !== 'unnestBlockButton')
  return <FormattingToolbar blockTypeSelectItems={[]}>{items}</FormattingToolbar>
}

type CommentEditorProps = ComponentProps['Comments']['Editor'] & {
  /** A composer field (2 to 8 lines, then it scrolls) with the placeholder for this kind; omitted: a plain body. */
  kind?: 'new' | 'reply' | 'edit'
}

export const CommentEditor = forwardRef<HTMLDivElement, CommentEditorProps>(function CommentEditor(props, ref) {
  const { className, onFocus, onBlur, autoFocus, editor, editable, kind } = props
  const context = useBlockNoteContext()
  const candidates = use(MentionCandidatesContext)
  const commentEditor = editor as unknown as CommentEditorInstance
  return (
    <ThemedBlockNoteView
      autoFocus={autoFocus}
      data-slot="comment-editor"
      data-kind={kind}
      className={cn(
        // App text at 13px, without BlockNote's editor padding and fill.
        '[&_.bn-editor]:bg-transparent! [&_.bn-editor]:p-0! [&_.bn-editor]:[font-family:inherit]! [&_.bn-editor]:text-[13px]/5! [&_.bn-editor]:text-foreground!',
        '[&_.bn-block-content]:p-0! [&_.bn-block-outer+.bn-block-outer]:mt-1!',
        // Static placeholders (BlockNote's come through a runtime <style>, which the production CSP blocks): none on
        // later empty lines, the field's hint on an empty field.
        '[&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-empty-and-focused]:not([data-is-only-empty-block]):has(.ProseMirror-trailingBreak:only-child)]:after:content-none!',
        kind &&
          '[&_.bn-editor]:box-border [&_.bn-editor]:max-h-44 [&_.bn-editor]:min-h-14 [&_.bn-editor]:overflow-y-auto [&_.bn-editor]:px-2.5! [&_.bn-editor]:py-2!',
        kind &&
          '[&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-only-empty-block]:has(.ProseMirror-trailingBreak:only-child)]:after:text-muted-foreground! [&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-only-empty-block]:has(.ProseMirror-trailingBreak:only-child)]:after:not-italic!',
        kind === 'new' &&
          "[&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-only-empty-block]:has(.ProseMirror-trailingBreak:only-child)]:after:content-['Add_a_comment…_Use_@_to_mention']!",
        kind === 'reply' &&
          "[&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-only-empty-block]:has(.ProseMirror-trailingBreak:only-child)]:after:content-['Reply…']!",
        kind === 'edit' &&
          "[&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-only-empty-block]:has(.ProseMirror-trailingBreak:only-child)]:after:content-['Edit_comment…']!",
        className,
      )}
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
    </ThemedBlockNoteView>
  )
})
