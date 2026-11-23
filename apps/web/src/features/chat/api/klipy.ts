import { keepPreviousData, useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { getChatKlipy, listChatKlipy } from '@orbit/api/generated/sdk.gen'
import type { KlipyItem, KlipyPage } from '@orbit/api/generated/types.gen'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { useDebouncedValue } from '@/lib/useDebouncedValue'

/** The two catalogs of KLIPY that chat offers. */
export type KlipyCatalog = 'gifs' | 'stickers'

/** The search asks KLIPY once the words have stayed the same this long. */
export const KLIPY_SEARCH_DELAY_MS = 300
/** The server serves pages 1 to 50 of a list. */
const KLIPY_LAST_PAGE = 50

/** Whether chat members can send GIFs and stickers from KLIPY: an admin set a KLIPY API key. */
export function useKlipyStatus(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.klipy.status(workspaceId),
    queryFn: async () => {
      const { data } = await getChatKlipy({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      return data
    },
    // The key changes rarely; Admin invalidates this when it saves or removes the key.
    staleTime: 60 * 60_000,
  })
}

/** The page after `page`, when KLIPY has one and the server serves it. */
export function nextKlipyPage(last: KlipyPage, page: number): number | undefined {
  return last.has_next && page < KLIPY_LAST_PAGE ? page + 1 : undefined
}

/** The items of all pages in order. A trending list can move between two pages: an item shows only once. */
export function klipyItems(pages: readonly KlipyPage[] | undefined): KlipyItem[] {
  const seen = new Set<string>()
  const items: KlipyItem[] = []
  for (const page of pages ?? []) {
    for (const item of page.items) {
      if (seen.has(item.id)) continue
      seen.add(item.id)
      items.push(item)
    }
  }
  return items
}

/**
 * GIFs or stickers from KLIPY: what is trending while `search` is blank, else the results for it. The search waits
 * for the typing to stop; meanwhile the last results stay. `fetchNextPage` loads one more page.
 */
export function useKlipySearch(workspaceId: string, catalog: KlipyCatalog, search: string, enabled = true) {
  const query = useDebouncedValue(search.trim(), KLIPY_SEARCH_DELAY_MS)
  const result = useInfiniteQuery({
    queryKey: queryKeys.klipy.search(workspaceId, catalog, query),
    queryFn: async ({ pageParam }) => {
      const { data } = await listChatKlipy({
        client: apiClient,
        path: { workspace_id: workspaceId, catalog },
        query: { q: query || undefined, page: pageParam },
        throwOnError: true,
      })
      return data
    },
    initialPageParam: 1,
    getNextPageParam: (last, _pages, page) => nextKlipyPage(last, page),
    staleTime: 5 * 60_000,
    placeholderData: keepPreviousData,
    enabled,
  })
  return { ...result, items: klipyItems(result.data?.pages) }
}
