import { useState } from 'react'
import { Emoji } from '@/components/common/Emoji'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

/** Emoji of a new callout (and of imported Notion callouts whose icon is not an emoji). */
export const CALLOUT_DEFAULT_EMOJI = '💡'
/** Background of a new callout; any BlockNote color name, `default` = no fill, just a border. */
export const CALLOUT_DEFAULT_BACKGROUND = 'gray'

interface CalloutEditor {
  readonly isEditable: boolean
  updateBlock: (id: string, update: { props: { emoji: string } }) => unknown
  setTextCursorPosition: (id: string, placement: 'start' | 'end') => void
  focus: () => void
}

interface CalloutViewProps {
  block: { id: string; props: { emoji: string } }
  editor: CalloutEditor
  contentRef: (node: HTMLElement | null) => void
}

/**
 * `render` of the `callout` block (Notion-style): the emoji on the left opens the app's emoji picker, the rich
 * text sits on the right. The rounded box, colors and nested children are styled from PageEditor's wrapper.
 */
export function CalloutView({ block, editor, contentRef }: CalloutViewProps) {
  const [open, setOpen] = useState(false)
  const emoji = block.props.emoji || CALLOUT_DEFAULT_EMOJI

  const pick = (next: string) => {
    setOpen(false)
    editor.updateBlock(block.id, { props: { emoji: next } })
    editor.setTextCursorPosition(block.id, 'end')
    editor.focus()
  }

  return (
    <div className="flex w-full min-w-0 items-start gap-2">
      <Popover open={open} onOpenChange={(next) => setOpen(next && editor.isEditable)} modal={false}>
        <PopoverTrigger
          render={
            <button
              type="button"
              contentEditable={false}
              className="flex size-6 shrink-0 cursor-pointer items-center justify-center rounded-[4px] text-[18px] leading-none transition-colors duration-150 ease-out select-none hover:bg-foreground/8 focus-visible:outline-2 focus-visible:outline-ring/50 in-[.bn-editor[contenteditable=false]]:pointer-events-none in-[.bn-editor[contenteditable=false]]:cursor-default"
              aria-label="Change callout icon"
              onMouseDown={(event) => {
                // Keep ProseMirror from turning the click into a selection change or a drag start.
                event.stopPropagation()
              }}
            />
          }
        >
          <Emoji value={emoji} size={20} />
        </PopoverTrigger>
        <PopoverContent align="start" className="w-auto gap-0 p-0">
          <EmojiPicker onPick={pick} />
        </PopoverContent>
      </Popover>
      <div ref={contentRef} className="min-w-0 flex-1 py-px" />
    </div>
  )
}

/** Clipboard / external HTML: the emoji followed by the text, in a plain box other apps can paste. */
export function CalloutExternalView({ block, contentRef }: Omit<CalloutViewProps, 'editor'>) {
  const emoji = block.props.emoji || CALLOUT_DEFAULT_EMOJI
  return (
    <div data-callout-emoji={emoji}>
      <span>{emoji} </span>
      <span ref={contentRef} />
    </div>
  )
}
