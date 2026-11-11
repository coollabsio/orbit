import { useState } from 'react'
import { ArrowRight } from 'reicon-react'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

const PAGE_SIZES = [10, 25, 50, 100]
// `items` lets Select.Value render the option label instead of the raw value.
const PAGE_SIZE_OPTIONS = PAGE_SIZES.map((size) => ({ value: String(size), label: String(size) }))

/** Client-side paging of a list. `visible` holds the rows of the current page. */
export function usePagedList<T>(items: T[], initialPerPage = 10) {
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(initialPerPage)
  const lastPage = Math.max(1, Math.ceil(items.length / perPage))
  const currentPage = Math.min(page, lastPage)
  return {
    visible: items.slice((currentPage - 1) * perPage, currentPage * perPage),
    pager: { total: items.length, page: currentPage, lastPage, perPage, setPage, setPerPage },
    resetPage: () => setPage(1),
  }
}

type PagerState = ReturnType<typeof usePagedList>['pager']

/** Footer with the row range, the page size and the previous and next buttons. */
export function ListPager({ total, page, lastPage, perPage, setPage, setPerPage }: PagerState) {
  const firstRow = total === 0 ? 0 : (page - 1) * perPage + 1
  const lastRow = Math.min(page * perPage, total)
  return (
    <footer className="flex min-h-11 items-center justify-between border-t px-4 text-[11px] text-muted-foreground/70">
      <div className="inline-flex h-7 items-center gap-3 whitespace-nowrap tabular-nums">
        <span>
          {firstRow}-{lastRow} of {total}
        </span>
        <Select
          items={PAGE_SIZE_OPTIONS}
          value={String(perPage)}
          onValueChange={(value) => {
            setPerPage(Number(value))
            setPage(1)
          }}
        >
          <SelectTrigger aria-label="Items per page" size="sm" className="w-16 px-2">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PAGE_SIZE_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
          </SelectContent>
        </Select>
      </div>
      <div className="flex items-center gap-1">
        <Tip label="Previous page">
          <Button type="button" variant="outline" size="icon-sm" aria-label="Previous page" disabled={page <= 1} onClick={() => setPage(page - 1)}>
            <ArrowRight className="size-3.5 rotate-180" />
          </Button>
        </Tip>
        <Tip label="Next page">
          <Button type="button" variant="outline" size="icon-sm" aria-label="Next page" disabled={page >= lastPage} onClick={() => setPage(page + 1)}>
            <ArrowRight className="size-3.5" />
          </Button>
        </Tip>
      </div>
    </footer>
  )
}
