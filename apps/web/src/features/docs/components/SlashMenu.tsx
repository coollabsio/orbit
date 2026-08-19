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
    <div className="slash-menu" ref={listRef}>
      {items.map((item, index) => {
        const showGroup = index === 0 || item.group !== items[index - 1]?.group
        return (
          <div key={`${item.group}-${item.title}`}>
            {showGroup ? <div className="slash-menu-group">{item.group}</div> : null}
            <button
              type="button"
              className="slash-menu-item"
              data-selected={index === selectedIndex ? 'true' : undefined}
              // preventDefault keeps focus in the textarea (the reference editor does the same)
              onMouseDown={(e) => {
                e.preventDefault()
                onSelect(item)
              }}
              onMouseEnter={() => onHover(index)}
            >
              <span className="slash-menu-icon">
                <item.icon size={18} />
              </span>
              <span className="slash-menu-text">
                <span className="slash-menu-title">{item.title}</span>
                <span className="slash-menu-subtext">{item.subtext}</span>
              </span>
            </button>
          </div>
        )
      })}
    </div>
  )
}
