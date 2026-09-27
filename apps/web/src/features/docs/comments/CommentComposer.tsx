// A comment field that looks and behaves like the app's textarea: a BlockNote comment editor (for mentions and
// formatting) inside a bordered, focus-ringed box that grows from two to eight lines, with an "@" button, Cancel and a
// primary submit button below. Cmd/Ctrl+Enter submits (Enter is a new line), Escape cancels.
import { useEffect, useState, type KeyboardEvent } from 'react'
import { SuggestionMenu, type BlockNoteEditor } from '@blocknote/core'
import { CommentEditorSubmitExtension } from '@blocknote/core/comments'
import { useEditorState } from '@blocknote/react'
import { AtSign } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { CommentEditor } from './CommentEditor'

export type AnyCommentEditor = BlockNoteEditor<any, any, any>

const MOD = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘' : 'Ctrl'

export function CommentComposer({
  editor,
  kind,
  submitLabel,
  onCancel,
  autoFocus = false,
  mentions = true,
  label,
  className,
}: {
  /** Needs `CommentEditorSubmitExtension` (its callback does the actual save). */
  editor: AnyCommentEditor
  /** Picks the placeholder (static CSS, see PageEditor.css). */
  kind: 'new' | 'reply' | 'edit'
  submitLabel: string
  /** Cancel button and Escape; without it neither is offered. */
  onCancel?: () => void
  autoFocus?: boolean
  /** Offer the "@" button (the picker itself is always on in comment editors). */
  mentions?: boolean
  /** Accessible name of the field. */
  label: string
  className?: string
}) {
  const isEmpty = useEditorState({ editor, selector: ({ editor }) => editor.isEmpty })
  const [busy, setBusy] = useState(false)
  // Also when an existing comment switches to editing (BlockNote's own `autoFocus` only acts on mount).
  useEffect(() => {
    if (autoFocus) editor.focus()
  }, [autoFocus, editor])

  const submit = async () => {
    const extension = editor.getExtension(CommentEditorSubmitExtension)
    if (!extension || busy) return
    setBusy(true)
    try {
      await extension.submit()
      // The button just turned disabled, which drops focus silently; keep typing in the (now empty) field.
      if (editor.isEditable && editor.domElement?.isConnected) editor.focus()
    } catch {
      // The thread store already reported the failure; the text stays so it can be sent again.
    } finally {
      setBusy(false)
    }
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'Escape' || !onCancel) return
    // Escape closes an open "@" picker first.
    if (editor.getExtension(SuggestionMenu)?.shown()) return
    event.preventDefault()
    // Keep BlockNote's popover from treating it as "dismiss" (that asks before discarding typed text).
    event.stopPropagation()
    onCancel()
  }

  const openMentionPicker = () => {
    editor.getExtension(SuggestionMenu)?.openSuggestionMenu('@', { deleteTriggerCharacter: true, ignoreQueryLength: true })
  }

  return (
    <div className={cn('flex flex-col gap-2', className)} data-comment-composer="" onKeyDown={onKeyDown}>
      <div
        className="orbit-comment-field rounded-lg border border-input bg-transparent transition-colors focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50 dark:bg-input/30"
        role="group"
        aria-label={label}
      >
        <CommentEditor editor={editor} editable autoFocus={autoFocus} className={`orbit-comment-input orbit-comment-input--${kind}`} />
      </div>
      <div className="flex items-center gap-1">
        {mentions ? (
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  className="size-7 text-muted-foreground"
                  aria-label="Mention someone"
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={openMentionPicker}
                />
              }
            >
              <AtSign className="size-4" />
            </TooltipTrigger>
            <TooltipContent>Mention someone</TooltipContent>
          </Tooltip>
        ) : null}
        <span className="flex-1" />
        {onCancel ? (
          <Button type="button" variant="ghost" size="sm" className="text-muted-foreground" onClick={onCancel}>
            Cancel
          </Button>
        ) : null}
        <Button type="button" size="sm" disabled={isEmpty || busy} title={`${submitLabel} (${MOD}+Enter)`} onClick={() => void submit()}>
          {submitLabel}
        </Button>
      </div>
    </div>
  )
}
