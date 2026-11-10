import { useLayoutEffect, useRef, useState, type ComponentProps } from 'react'
import { DocumentText } from 'reicon-react'
import { cn } from 'cn'
import { Checkbox } from '@/components/ui/checkbox'
import { Emoji } from '@/components/common/Emoji'
import { UserAvatar } from '@/components/common/UserAvatar'
import { cellOf, displayText, options } from '@orbit/database/values'
import type { Property, Row, StatusOption } from '@orbit/database/types'
import { useDatabaseEnv } from '../context'
import { isComputed } from '@orbit/database/meta'
import { ButtonCell } from './ButtonCell'
import { Pill } from './Pill'

interface CellViewProps {
  property: Property
  row: Row
  width: number
  wrap: boolean
  /** Checkbox cells toggle in place. */
  onToggle?: () => void
}

/** What a cell shows when it is not being edited (spec 7.3). */
export function CellView({ property, row, width, wrap, onToggle }: CellViewProps) {
  const { ctx } = useDatabaseEnv()
  const value = cellOf(row, property)
  switch (property.type) {
    case 'title':
      return (
        <span className={cn('flex min-w-0 items-center gap-1.5 font-medium', wrap ? 'items-start' : '')}>
          {row.icon ? <Emoji value={row.icon} className="shrink-0" /> : null}
          <Text wrap={wrap} muted={!row.title}>{row.title || 'Untitled'}</Text>
          {row.has_content ? <DocumentText className="size-3.5 shrink-0 text-muted-foreground" aria-label="Has text" /> : null}
        </span>
      )
    case 'number':
      return <span className="block w-full truncate text-right tabular-nums">{displayText(property, value, ctx)}</span>
    case 'select':
    case 'status':
      return value?.kind === 'choice' ? <OptionPill property={property} id={value.id} /> : null
    case 'multi_select':
      return value?.kind === 'choices' ? <OptionPills property={property} ids={value.ids} width={width} wrap={wrap} /> : null
    case 'person':
    case 'created_by':
    case 'last_edited_by':
      return value?.kind === 'people' ? <People ids={value.ids} width={width} wrap={wrap} muted={isComputed(property.type)} /> : null
    case 'checkbox':
      return (
        <span className="flex w-full justify-center">
          <Checkbox
            checked={value !== null}
            aria-label={property.name}
            tabIndex={-1}
            disabled={!onToggle}
            onClick={(event) => event.stopPropagation()}
            onCheckedChange={() => onToggle?.()}
          />
        </span>
      )
    case 'url':
      return value?.kind === 'url' ? <UrlLink url={value.url} wrap={wrap} /> : null
    case 'created_time':
    case 'last_edited_time':
      return <Text wrap={wrap} muted>{displayText(property, value, ctx)}</Text>
    case 'text':
    case 'date':
      return <Text wrap={wrap}>{displayText(property, value, ctx)}</Text>
    case 'button':
      return <ButtonCell property={property} row={row} />
  }
}

function Text({ wrap, muted = false, children }: { wrap: boolean; muted?: boolean; children: string }) {
  return (
    <span className={cn('min-w-0', wrap ? 'break-words whitespace-pre-wrap' : 'truncate', muted && 'text-muted-foreground')}>{children}</span>
  )
}

export function OptionPill({ property, id, ...props }: { property: Property; id: string } & Omit<ComponentProps<'span'>, 'property' | 'color' | 'id'>) {
  const option = options(property).find((candidate) => candidate.id === id)
  if (!option) return null
  return (
    <Pill color={option.color} dot={property.type === 'status'} data-group={(option as StatusOption).group} {...props}>
      {option.name}
    </Pill>
  )
}

/** Room the "+N" count keeps at the end of a line that does not show every item. */
const COUNT_PX = 28
/** The cell's horizontal padding (`px-2`). */
const CELL_PADDING_PX = 16

/**
 * How many items of a one-line cell fit its width, measured from the rendered items (`data-fit-item`, each with its
 * text as the last child). Items past the count stay rendered but out of the flow (`data-overflow`), so a wider column
 * can measure them again. `fitKey` names what the items show; only visible rows render cells, so this stays cheap.
 */
function useFit(count: number, fitKey: string, width: number, wrap: boolean) {
  const ref = useRef<HTMLSpanElement>(null)
  const [shown, setShown] = useState(count)
  useLayoutEffect(() => {
    const box = ref.current
    if (wrap || !box) return setShown(count)
    const items = [...box.querySelectorAll<HTMLElement>(':scope > [data-fit-item]')]
    const gap = parseFloat(getComputedStyle(box).columnGap) || 0
    const room = width - CELL_PADDING_PX
    let used = 0
    let fit = 0
    for (const [index, item] of items.entries()) {
      // A truncated text (the first item may shrink) still counts at its full width.
      const text = item.lastElementChild
      const natural = item.offsetWidth + (text ? text.scrollWidth - text.clientWidth : 0)
      const reserve = index + 1 < items.length ? COUNT_PX : 0
      if (fit > 0 && used + natural + reserve > room) break
      used += natural + gap
      fit += 1
    }
    setShown(fit)
  }, [count, fitKey, width, wrap])
  return { ref, shown: wrap ? count : shown }
}

/** Classes of a fitted item: the first one shrinks (and truncates) when even it does not fit; the rest never do. */
const fitItem = (index: number) => cn(index === 0 ? 'shrink' : 'shrink-0', 'data-overflow:invisible data-overflow:absolute data-overflow:top-0 data-overflow:left-0')

function FitCount({ hidden }: { hidden: number }) {
  return hidden > 0 ? <span className="shrink-0 text-xs text-muted-foreground">+{hidden}</span> : null
}

function OptionPills({ property, ids, width, wrap }: { property: Property; ids: string[]; width: number; wrap: boolean }) {
  const list = options(property)
  const fitKey = ids.map((id) => list.find((option) => option.id === id)?.name ?? '').join('\n')
  const { ref, shown } = useFit(ids.length, fitKey, width, wrap)
  return (
    <span ref={ref} className={cn('flex min-w-0 items-center gap-1', wrap ? 'flex-wrap py-1' : 'overflow-hidden')}>
      {ids.map((id, index) => (
        <OptionPill key={id} property={property} id={id} data-fit-item="" data-overflow={index >= shown || undefined} className={fitItem(index)} />
      ))}
      <FitCount hidden={ids.length - shown} />
    </span>
  )
}

function People({ ids, width, wrap, muted }: { ids: string[]; width: number; wrap: boolean; muted: boolean }) {
  const { users } = useDatabaseEnv()
  const fitKey = ids.map((id) => users.get(id)?.name ?? '').join('\n')
  const { ref, shown } = useFit(ids.length, fitKey, width, wrap)
  return (
    <span ref={ref} className={cn('flex min-w-0 items-center gap-2', wrap ? 'flex-wrap py-1' : 'overflow-hidden', muted && 'text-muted-foreground')}>
      {ids.map((id, index) => {
        const user = users.get(id)
        return (
          <span
            key={id}
            data-fit-item=""
            data-overflow={index >= shown || undefined}
            className={cn('flex min-w-0 items-center gap-1.5', fitItem(index))}
          >
            <UserAvatar user={user} name={user ? undefined : '?'} size={18} />
            <span className="truncate">{user?.name ?? 'Former member'}</span>
          </span>
        )
      })}
      <FitCount hidden={ids.length - shown} />
    </span>
  )
}

function UrlLink({ url, wrap }: { url: string; wrap: boolean }) {
  let label = url
  try {
    const parsed = new URL(url)
    label = `${parsed.host}${parsed.pathname === '/' ? '' : parsed.pathname}`
  } catch {
    // shown as stored
  }
  return (
    <a
      href={url}
      target="_blank"
      rel="noreferrer"
      className={cn('min-w-0 text-foreground underline decoration-foreground/30 underline-offset-2 hover:decoration-foreground', wrap ? 'break-all' : 'truncate')}
      onClick={(event) => event.stopPropagation()}
    >
      {label}
    </a>
  )
}
