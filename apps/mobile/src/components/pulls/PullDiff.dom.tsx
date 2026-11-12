'use dom'

// One file's diff, rendered by Pierre Diffs in a web view (the web viewer uses the same library). The screen around
// it is native: a line or a thread here calls a native action. Unified only: a phone is too narrow for split.
import type { CommentTarget } from '@orbit/api/github'
import { PatchDiff, type DiffLineAnnotation, type SelectedLineRange } from '@pierre/diffs/react'
import type { DOMProps } from 'expo/dom'
import { useMemo, useRef } from 'react'

/** What a thread shows in the diff (it crosses the bridge as JSON). */
export interface DiffThread {
  id: string
  side: 'LEFT' | 'RIGHT'
  line: number
  resolved: boolean
  comments: { id: string; author: string; body: string }[]
}

export interface PullDiffProps {
  path: string
  patch: string
  threads: DiffThread[]
  theme: 'light' | 'dark'
  background: string
  foreground: string
  muted: string
  border: string
  onThreadPress: (threadId: string) => Promise<void>
  onLinesSelected: (target: CommentTarget) => Promise<void>
  dom?: DOMProps
}

const toSide = (side: 'additions' | 'deletions' | undefined): 'LEFT' | 'RIGHT' => (side === 'deletions' ? 'LEFT' : 'RIGHT')

export default function PullDiff({ path, patch, threads, theme, background, foreground, muted, border, onThreadPress, onLinesSelected }: PullDiffProps) {
  const actions = useRef({ onThreadPress, onLinesSelected })
  actions.current = { onThreadPress, onLinesSelected }
  const annotations = useMemo<DiffLineAnnotation<DiffThread>[]>(
    () => threads.map((thread) => ({ side: thread.side === 'LEFT' ? 'deletions' : 'additions', lineNumber: thread.line, metadata: thread })),
    [threads],
  )
  const options = useMemo(
    () => ({
      diffStyle: 'unified' as const,
      overflow: 'wrap' as const,
      themeType: theme,
      hunkSeparators: 'line-info' as const,
      enableLineSelection: true,
      onLineSelected: (range: SelectedLineRange | null) => {
        if (!range) return
        const top = Math.min(range.start, range.end)
        const bottom = Math.max(range.start, range.end)
        const side = toSide(range.endSide ?? range.side)
        const target: CommentTarget = { path, side, line: bottom }
        if (top !== bottom) Object.assign(target, { start_line: top, start_side: toSide(range.side) })
        void actions.current.onLinesSelected(target)
      },
    }),
    [theme, path],
  )
  return (
    // the web view's #root is a flex row: the root must grow (see .ai/lessons.md)
    <div style={{ flex: 1, minHeight: '100%', background, color: foreground, fontFamily: '-apple-system, system-ui, sans-serif' }}>
      <PatchDiff<DiffThread, undefined>
        patch={patch}
        options={options}
        lineAnnotations={annotations}
        renderAnnotation={(annotation) =>
          annotation.metadata ? (
            <button
              type="button"
              onClick={() => void actions.current.onThreadPress(annotation.metadata!.id)}
              style={{ display: 'block', width: 'calc(100% - 16px)', margin: '4px 8px', padding: '8px 10px', textAlign: 'left', borderRadius: 8, border: `1px solid ${border}`, background, color: foreground, font: 'inherit', fontSize: 13 }}
            >
              {annotation.metadata.comments.slice(0, 2).map((comment) => (
                <span key={comment.id} style={{ display: 'block', marginBottom: 4 }}>
                  <strong>{comment.author}</strong> {comment.body.length > 180 ? `${comment.body.slice(0, 180)}…` : comment.body}
                </span>
              ))}
              <span style={{ color: muted, fontSize: 12 }}>
                {annotation.metadata.comments.length > 2 ? `${annotation.metadata.comments.length - 2} more · ` : ''}
                {annotation.metadata.resolved ? 'Resolved · ' : ''}Tap to open
              </span>
            </button>
          ) : null
        }
      />
    </div>
  )
}
