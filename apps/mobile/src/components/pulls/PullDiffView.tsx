import type { GithubReviewThread } from '@orbit/api/generated/types.gen'
import type { CommentTarget } from '@orbit/api/github'
import { memo, useEffect, useMemo, useRef } from 'react'

import { whileMounted } from '../../data/page-editor'
import { markdownToText } from '../../data/markdown'
import { useTheme } from '../../theme'
import PullDiff, { type DiffThread } from './PullDiff.dom'

/**
 * One file's diff with what it needs from the app: the theme and the file's threads. Every render sends all props to
 * the web view, so it renders only when they change; the callbacks are read through a ref.
 */
export const PullDiffView = memo(function PullDiffView({ path, patch, threads, onThreadPress, onLinesSelected }: {
  path: string
  patch: string
  threads: GithubReviewThread[]
  onThreadPress: (threadId: string) => void
  onLinesSelected: (target: CommentTarget) => void
}) {
  const { scheme, colors } = useTheme()
  const latest = useRef({ onThreadPress, onLinesSelected })
  useEffect(() => {
    latest.current = { onThreadPress, onLinesSelected }
  })
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])
  const shown = useMemo<DiffThread[]>(
    () =>
      threads
        .filter((thread) => thread.path === path && thread.line != null)
        .map((thread) => ({
          id: thread.id,
          side: thread.side === 'LEFT' ? 'LEFT' : 'RIGHT',
          line: thread.line as number,
          resolved: thread.is_resolved,
          comments: thread.comments.map((comment) => ({ id: comment.id, author: comment.author.login, body: markdownToText(comment.body) })),
        })),
    [threads, path],
  )
  const actions = useMemo(
    () => ({
      onThreadPress: whileMounted(mounted, async (id: string) => latest.current.onThreadPress(id)),
      onLinesSelected: whileMounted(mounted, async (target: CommentTarget) => latest.current.onLinesSelected(target)),
    }),
    [],
  )
  const dom = useMemo(() => ({ style: { flex: 1 } }), [])
  return (
    <PullDiff
      dom={dom}
      path={path}
      patch={patch}
      threads={shown}
      theme={scheme}
      background={colors.background}
      foreground={colors.foreground}
      muted={colors.mutedForeground}
      border={colors.border}
      onThreadPress={actions.onThreadPress}
      onLinesSelected={actions.onLinesSelected}
    />
  )
})
