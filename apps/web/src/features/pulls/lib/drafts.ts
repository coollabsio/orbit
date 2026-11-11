import { useEffect, useRef, useState } from 'react'
import type { CommentTarget, PullRef } from '@orbit/api/github'

const prefix = (workspaceId: string, ref: PullRef) => `orbit:pulls:draft:${workspaceId}:${ref.owner}/${ref.repo}#${ref.number}`
const lineKey = (side: 'LEFT' | 'RIGHT', line: number) => `${side[0]}${line}`

/** Where a comment draft is kept: one key for each pull request, file and line range. */
export function draftKey(workspaceId: string, ref: PullRef, target: CommentTarget): string {
  const range = target.start_line != null && target.start_side ? `${lineKey(target.start_side, target.start_line)}-${lineKey(target.side, target.line)}` : lineKey(target.side, target.line)
  return `${prefix(workspaceId, ref)}:${target.path}:${range}`
}

export function replyDraftKey(workspaceId: string, ref: PullRef, threadId: string): string {
  return `${prefix(workspaceId, ref)}:reply:${threadId}`
}

function read(key: string): string {
  try { return localStorage.getItem(key) ?? '' } catch { return '' }
}

/** A comment draft that outlives the page: written 300 ms after typing stops, removed by `clear`. */
export function useDraft(key: string): [string, (value: string) => void, () => void] {
  const [value, setValue] = useState(() => read(key))
  const timer = useRef<number | undefined>(undefined)
  const pending = useRef<string | null>(null)
  const flush = () => {
    window.clearTimeout(timer.current)
    if (pending.current === null) return
    try { if (pending.current) localStorage.setItem(key, pending.current); else localStorage.removeItem(key) } catch { /* private mode: the draft lasts this page */ }
    pending.current = null
  }
  // write what is left when the composer closes
  useEffect(() => flush, [key]) // eslint-disable-line react-hooks/exhaustive-deps
  const change = (next: string) => {
    setValue(next)
    pending.current = next
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(flush, 300)
  }
  const clear = () => {
    window.clearTimeout(timer.current)
    pending.current = null
    setValue('')
    try { localStorage.removeItem(key) } catch { /* nothing kept */ }
  }
  return [value, change, clear]
}
