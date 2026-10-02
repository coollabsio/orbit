import { readFileSync } from 'node:fs'

/**
 * The emoji picker loads its data from Orbit's own copy of the data. In a test it comes from the `emojibase-data`
 * package: a fake `fetch` returns this for such a URL, and `null` means the URL is not an emoji data request.
 */
export function emojibaseResponse(input: RequestInfo | URL): Response | null {
  const file = /\/emojibase-[\d.]+\/en\/(data|messages)\.json/.exec(String(input instanceof Request ? input.url : input))?.[1]
  if (!file) return null
  return new Response(readFileSync(`node_modules/emojibase-data/en/${file}.json`), { headers: { 'content-type': 'application/json', etag: 'test' } })
}
