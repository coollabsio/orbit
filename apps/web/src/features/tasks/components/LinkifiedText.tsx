import type { ReactNode } from 'react'

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
    parts.push(
      <a key={start} href={url} target="_blank" rel="noreferrer" onClick={(event) => event.stopPropagation()}>
        {url}
      </a>,
    )
    parts.push(rawUrl.slice(url.length))
    cursor = start + rawUrl.length
  }
  parts.push(text.slice(cursor))

  return <>{parts}</>
}
