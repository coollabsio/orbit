import { NodeViewWrapper, type ReactNodeViewProps } from '@tiptap/react'
import { TaskMentionChip } from './TaskChip'

/** The chip inside the editor: same live status and title as the read-only view. */
export function TaskMentionNodeView({ node }: ReactNodeViewProps) {
  return (
    <NodeViewWrapper as="span" className="editor-chip-node" data-task-mention="">
      <TaskMentionChip node={node} />
    </NodeViewWrapper>
  )
}
