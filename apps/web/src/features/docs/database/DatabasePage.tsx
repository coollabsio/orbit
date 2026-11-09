import { useLayoutEffect, useState, type ReactNode } from 'react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { useDatabase } from './api'
import { useDatabaseEnvValue } from './context'
import { DatabaseSurface } from './DatabaseSurface'

interface DatabasePageProps {
  workspaceId: string
  databaseId: string
  /** The page lock: properties and views are read-only, rows and values stay writable. */
  locked: boolean
  /** The page cover, full width above the title column. */
  cover: ReactNode
  /** The icon and title controls of the page. */
  title: ReactNode
}

/** A database page (spec 7.1): the page header above the database's views, in one scroller both ways. */
export function DatabasePage({ workspaceId, databaseId, locked, cover, title }: DatabasePageProps) {
  const query = useDatabase(workspaceId, databaseId)
  const env = useDatabaseEnvValue(workspaceId, databaseId, locked)
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null)
  const [viewport, setViewport] = useState(0)

  // The header column stays put while the table scrolls sideways: it is as wide as the scroller's content box.
  // `left-0` keeps it at the content edge: a sticky offset already starts inside the scroller's padding.
  useLayoutEffect(() => {
    if (!scroller) return
    const measure = () => {
      const style = getComputedStyle(scroller)
      setViewport(scroller.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight))
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(scroller)
    return () => observer.disconnect()
  }, [scroller])

  const header = (children: ReactNode) => (
    <div className="sticky left-0" style={{ width: viewport || undefined }}>
      {cover}
      <div className="px-[30px]">
        {title}
        {children}
      </div>
    </div>
  )

  const database = query.data
  if (!database) {
    return (
      <DatabaseScroller setScroller={setScroller}>
        {header(
          query.isError ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              The database could not load.
              <Button type="button" variant="outline" size="sm" onClick={() => void query.refetch()}>
                Retry
              </Button>
            </div>
          ) : (
            <Spinner className="text-muted-foreground" />
          ),
        )}
      </DatabaseScroller>
    )
  }

  return (
    <DatabaseSurface
      database={database}
      views={database.views}
      env={env}
      locked={locked}
      viewKey={`orbit:database-view:${databaseId}`}
      page={{ scrollElement: scroller }}
      layout={({ controls, content, board }) => (
        <DatabaseScroller setScroller={setScroller}>
          {header(controls)}
          {/* The row drag handles and checkboxes (40 px) sit left of the table, in this padding plus the scroller's
              (54 px in all, 42 px below 900 px). */}
          <div className={cn('px-[30px]', board ? 'pt-3' : 'pt-2')}>{content}</div>
        </DatabaseScroller>
      )}
    />
  )
}

/** The page's one scroller, both ways: the header sticks to its left edge, the table header to its top. */
function DatabaseScroller({ setScroller, children }: { setScroller: (element: HTMLDivElement | null) => void; children: ReactNode }) {
  return (
    <div
      ref={setScroller}
      data-print-root=""
      data-virtual-scroller=""
      className="min-h-0 flex-1 overflow-auto px-6 pt-8 pb-24 max-[899px]:px-3 max-[899px]:pt-[18px] max-[899px]:pb-14"
    >
      <div className="w-max min-w-full">{children}</div>
    </div>
  )
}
