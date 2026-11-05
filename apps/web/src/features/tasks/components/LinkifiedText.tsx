import type { ReactNode } from 'react'
import { appNavigate, internalPath, isPlainClick } from '@/lib/navigateBridge'

const URL_PATTERN = /https?:\/\/[^\s]+/g
const TRAILING_PUNCTUATION = /[.,!?;:)}\]]+$/

export function LinkifiedText({ text }: { text: string }) {
  const parts: ReactNode[] = []
  let cursor = 0

  for (const match of text.matchAll(URL_PATTERN)) {
    const start = match.index
    const rawUrl = match[0]
    const url = rawUrl.replace(TRAILING_PUNCTUATION, '')
    parts.push(text.slice(cursor, start))
    const path = internalPath(url)
    parts.push(
      <a
        key={start}
        href={path ?? url}
        target={path ? undefined : '_blank'}
        rel={path ? undefined : 'noreferrer'}
        className="cursor-pointer text-primary underline underline-offset-2"
        onClick={(event) => {
          event.stopPropagation()
          if (!path || !isPlainClick(event)) return
          event.preventDefault()
          appNavigate(path)
        }}
      >
        {url}
      </a>,
    )
    parts.push(rawUrl.slice(url.length))
    cursor = start + rawUrl.length
  }
  parts.push(text.slice(cursor))

  return <>{parts}</>
}
