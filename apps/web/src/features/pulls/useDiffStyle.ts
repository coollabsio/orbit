import { useCallback, useState } from 'react'

export type DiffStyle = 'split' | 'unified'
const KEY = 'orbit:pulls:diff-style'

function read(): DiffStyle {
  try { return localStorage.getItem(KEY) === 'unified' ? 'unified' : 'split' } catch { return 'split' }
}

/** Split or unified diffs; the choice stays for the next pull request. */
export function useDiffStyle(): [DiffStyle, (style: DiffStyle) => void] {
  const [style, setStyle] = useState<DiffStyle>(read)
  const change = useCallback((next: DiffStyle) => {
    setStyle(next)
    try { localStorage.setItem(KEY, next) } catch { /* private mode: the choice lasts this page */ }
  }, [])
  return [style, change]
}
