import { BlockNoteEditor as BlockNoteEditorClass, type BlockNoteEditor } from '@blocknote/core'
import { createEditorSchemas, type TaskEditorSchema } from '@orbit/editor/schema'
import { CalloutExternalView, CalloutView } from './CalloutBlock'
import { codeBlockOptions } from './codeBlock'
import { PageBlockExternalView, PageBlockView } from './PageBlockCard'
import { PageMentionView } from './PageMentionChip'

export const { pageEditorSchema, taskEditorSchema } = createEditorSchemas({
  page: { render: PageBlockView, toExternalHTML: PageBlockExternalView },
  callout: { render: CalloutView, toExternalHTML: CalloutExternalView },
  mention: { render: PageMentionView },
  codeBlock: codeBlockOptions,
})

/** Every block type the task editor accepts. */
export const TASK_EDITOR_BLOCK_TYPES: ReadonlySet<string> = new Set(Object.keys(taskEditorSchema.blockSchema))

let markdownParser: BlockNoteEditor<TaskEditorSchema['blockSchema'], TaskEditorSchema['inlineContentSchema'], TaskEditorSchema['styleSchema']> | null = null

/** Blocks for task text that exists only as markdown (written before rich text); `@Name` stays text. */
export function markdownToTaskBlocks(markdown: string): unknown[] {
  markdownParser ??= BlockNoteEditorClass.create({ schema: taskEditorSchema, _tiptapOptions: { injectCSS: false } })
  return markdownParser.tryParseMarkdownToBlocks(markdown)
}

/** Every block type the editor accepts; stored content with other types is dropped on load. */
export const EDITOR_BLOCK_TYPES: ReadonlySet<string> = new Set(Object.keys(pageEditorSchema.blockSchema))
