// the reference editor-style suggestion menu: grouped items, icon tile + title + subtext,
// selected row highlighted; rendered below the editing block.
import { useEffect, useRef } from 'react'
import type { SlashItem } from '../slashItems'

export function SlashMenu({
  items,
  selectedIndex,
  onSelect,
  onHover,
}: {
  items: SlashItem[]
  selectedIndex: number
  onSelect: (item: SlashItem) => void
  onHover: (index: number) => void
}) {
  const listRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = listRef.current?.querySelector('[data-selected="true"]')
    el?.scrollIntoView({ block: 'nearest' })
  }, [selectedIndex])

  if (items.length === 0) return null

  return (
    <div
      className="absolute top-[calc(100%+6px)] left-[30px] z-30 max-h-[340px] w-[min(340px,calc(100vw-48px))] overflow-y-auto rounded-[12px] border border-border bg-card p-1.5 shadow-[0_20px_25px_-5px_rgba(0,0,0,0.4),0_8px_10px_-6px_rgba(0,0,0,0.4)]"
      ref={listRef}
    >
      {items.map((item, index) => {
        const showGroup = index === 0 || item.group !== items[index - 1]?.group
        return (
          <div key={`${item.group}-${item.title}`}>
            {showGroup ? (
              <div className="px-2 pt-[7px] pb-[5px] text-[11px] font-semibold tracking-[0.04em] text-muted-foreground/70 uppercase">
                {item.group}
              </div>
            ) : null}
            <button
              type="button"
              className="flex w-full items-center gap-2.5 rounded-[7px] px-2 py-[7px] text-left hover:bg-muted data-[selected=true]:bg-muted"
              data-selected={index === selectedIndex ? 'true' : undefined}
              // preventDefault keeps focus in the textarea (the reference editor does the same)
              onMouseDown={(e) => {
                e.preventDefault()
                onSelect(item)
              }}
              onMouseEnter={() => onHover(index)}
            >
              <span className="inline-flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-card text-muted-foreground">
                <item.icon className="size-[18px]" />
              </span>
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="text-[13px] font-medium text-foreground">{item.title}</span>
                <span className="text-[11px] text-muted-foreground/70">{item.subtext}</span>
              </span>
            </button>
          </div>
        )
      })}
    </div>
  )
}
