import type { KlipyItem } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { type KlipyCatalog, useKlipySearch } from '../../api/klipy'
import { masonryColumns } from './klipyLayout'

/**
 * GIFs (two columns, each at its own height) or stickers (four in a row) from KLIPY: what is trending, or the results
 * for `search`. A list that has more loads the next page when its end scrolls into view. Its scroll container is the
 * nearest `data-slot="klipy-scroll"`.
 */
export function KlipyResults({ workspaceId, catalog, search, onPick }: { workspaceId: string; catalog: KlipyCatalog; search: string; onPick: (item: KlipyItem) => void }) {
  const results = useKlipySearch(workspaceId, catalog, search)
  const gifs = catalog === 'gifs'
  const what = gifs ? 'GIFs' : 'stickers'

  if (results.isPending) {
    return (
      <div data-slot="klipy-loading" role="status" aria-busy="true" aria-label={`Loading ${what}`}>
        {gifs ? <GifPlaceholders /> : <StickerPlaceholders />}
      </div>
    )
  }
  if (results.isError && results.items.length === 0) {
    const off = results.error instanceof ApiProblem && results.error.code === 'klipy_not_configured'
    return (
      <div data-slot="klipy-error" role="alert" className="flex flex-col items-center gap-2 px-3 py-8 text-center text-sm text-muted-foreground">
        <p>{off ? `${gifs ? 'GIFs' : 'Stickers'} from KLIPY are turned off on this Orbit.` : `KLIPY did not answer. ${gifs ? 'GIFs' : 'Stickers'} could not load.`}</p>
        {off ? null : (
          <Button variant="outline" size="sm" onClick={() => void results.refetch()}>
            Try again
          </Button>
        )}
      </div>
    )
  }
  if (results.items.length === 0) {
    return <p className="px-1 py-8 text-center text-sm text-muted-foreground">{search.trim() ? `No ${what} match` : `No ${what} to show`}</p>
  }

  const tile = (item: KlipyItem) => (
    <button
      key={item.id}
      type="button"
      data-slot="klipy-item"
      aria-label={item.title || (gifs ? 'GIF' : 'Sticker')}
      className={
        gifs
          ? 'block w-full overflow-hidden rounded-md bg-muted outline-none hover:opacity-90 focus-visible:ring-3 focus-visible:ring-ring/50'
          : 'flex aspect-square items-center justify-center rounded-md p-1.5 outline-none hover:bg-muted focus-visible:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50'
      }
      onClick={() => onPick(item)}
    >
      <img
        src={item.preview.url}
        alt={item.title}
        // the size reserves the tile's box before the image loads
        width={item.preview.width || undefined}
        height={item.preview.height || undefined}
        loading="lazy"
        decoding="async"
        draggable={false}
        referrerPolicy="no-referrer"
        className={gifs ? 'block h-auto w-full' : 'size-full object-contain'}
      />
    </button>
  )

  return (
    <div data-slot="klipy-results" aria-busy={results.isFetching ? 'true' : undefined}>
      {gifs ? (
        <div className="flex items-start gap-1">
          {masonryColumns(results.items, 2).map((column, index) => (
            <div key={index} className="flex min-w-0 flex-1 flex-col gap-1">
              {column.map(tile)}
            </div>
          ))}
        </div>
      ) : (
        <div className="grid grid-cols-4 gap-1">{results.items.map(tile)}</div>
      )}
      {results.isFetchNextPageError ? (
        <div className="flex justify-center pt-2">
          <Button variant="outline" size="sm" onClick={() => void results.fetchNextPage()}>
            Load more
          </Button>
        </div>
      ) : results.hasNextPage ? (
        <LoadMore key={results.items.length} loading={results.isFetchingNextPage} onVisible={() => void results.fetchNextPage()} />
      ) : null}
      {results.isFetchingNextPage ? <div className="mt-1">{gifs ? <Skeleton className="h-20 w-full" /> : <StickerPlaceholders rows={1} />}</div> : null}
    </div>
  )
}

/**
 * An empty line at the end of a list that has more: it asks for the next page when it comes near the visible part of
 * the scroll container. It is mounted again after each page (its `key`), so it asks again while it still shows.
 */
function LoadMore({ loading, onVisible }: { loading: boolean; onVisible: () => void }) {
  return (
    <div
      aria-hidden="true"
      className="h-px"
      ref={(element) => {
        if (!element || loading || typeof IntersectionObserver === 'undefined') return
        const observer = new IntersectionObserver(
          (entries) => {
            if (entries.some((entry) => entry.isIntersecting)) {
              observer.disconnect()
              onVisible()
            }
          },
          { root: element.closest('[data-slot=klipy-scroll]'), rootMargin: '0px 0px 160px 0px' },
        )
        observer.observe(element)
        return () => observer.disconnect()
      }}
    />
  )
}

/** Tiles of mixed heights while the first page loads, as GIFs are. */
function GifPlaceholders() {
  return (
    <div className="flex items-start gap-1">
      <div className="flex flex-1 flex-col gap-1">
        <Skeleton className="h-24" />
        <Skeleton className="h-32" />
        <Skeleton className="h-20" />
      </div>
      <div className="flex flex-1 flex-col gap-1">
        <Skeleton className="h-30" />
        <Skeleton className="h-22" />
        <Skeleton className="h-28" />
      </div>
    </div>
  )
}

function StickerPlaceholders({ rows = 2 }: { rows?: number }) {
  return (
    <div className="grid grid-cols-4 gap-1">
      {Array.from({ length: rows * 4 }, (_, index) => (
        <Skeleton key={index} className="aspect-square" />
      ))}
    </div>
  )
}

/** The attribution that KLIPY asks for under its results. */
export function PoweredByKlipy() {
  return (
    <p data-slot="klipy-attribution" className="shrink-0 border-t px-3 py-1.5 text-right text-[0.6875rem] text-muted-foreground">
      Powered by KLIPY
    </p>
  )
}
