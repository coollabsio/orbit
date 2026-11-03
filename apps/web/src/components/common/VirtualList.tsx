import { useRef, type ComponentProps, type Key, type ReactNode } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { cn } from 'cn'
import { useScrollMemory } from '@/lib/useScrollMemory'

interface VirtualListProps extends Omit<ComponentProps<'div'>, 'children'> {
  count: number
  /** The height of a row before it is measured. */
  rowHeight: number
  rowKey: (index: number) => Key
  children: (index: number) => ReactNode
}

/** A scrolling list that keeps only the rows near the view in the DOM (TanStack Virtual). Rows are measured, so their heights can differ.
 *  It keeps its scroll position per page. */
function VirtualList({ count, rowHeight, rowKey, children, className, ...props }: VirtualListProps) {
  const scroller = useRef<HTMLDivElement>(null)
  const savedTop = useScrollMemory(scroller, 'virtual-list')
  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scroller.current,
    initialOffset: savedTop,
    getItemKey: rowKey,
    estimateSize: () => rowHeight,
    overscan: 10,
  })
  return (
    <div ref={scroller} data-slot="virtual-list" data-virtual-scroller className={cn('overflow-y-auto', className)} {...props}>
      <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((item) => (
          <div key={item.key} ref={virtualizer.measureElement} data-index={item.index} className="absolute inset-x-0" style={{ top: item.start }}>
            {children(item.index)}
          </div>
        ))}
      </div>
    </div>
  )
}

export { VirtualList }
