export type CursorPage<T> = {
  items: T[]
  next_cursor?: string | null
}

export async function fetchAllPages<T>(
  fetchPage: (cursor: string | undefined) => Promise<CursorPage<T>>,
): Promise<CursorPage<T>> {
  const items: T[] = []
  let cursor: string | undefined
  do {
    const page = await fetchPage(cursor)
    items.push(...page.items)
    cursor = page.next_cursor ?? undefined
  } while (cursor)
  return { items, next_cursor: null }
}
