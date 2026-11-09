import { DocumentText } from 'reicon-react'
import { cn } from 'cn'
import { Checkbox } from '@/components/ui/checkbox'
import { Emoji } from '@/components/common/Emoji'
import { UserAvatar } from '@/components/common/UserAvatar'
import { cellOf, displayText, options } from '@orbit/database/values'
import type { Property, Row, StatusOption } from '@orbit/database/types'
import { useDatabaseEnv } from '../context'
import { isComputed } from '../propertyMeta'
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
      return value?.kind === 'people' ? <People ids={value.ids} wrap={wrap} muted={isComputed(property.type)} /> : null
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
  }
}

function Text({ wrap, muted = false, children }: { wrap: boolean; muted?: boolean; children: string }) {
  return (
    <span className={cn('min-w-0', wrap ? 'break-words whitespace-pre-wrap' : 'truncate', muted && 'text-muted-foreground')}>{children}</span>
  )
}

export function OptionPill({ property, id, className }: { property: Property; id: string; className?: string }) {
  const option = options(property).find((candidate) => candidate.id === id)
  if (!option) return null
  return (
    <Pill color={option.color} dot={property.type === 'status'} className={className} data-group={(option as StatusOption).group}>
      {option.name}
    </Pill>
  )
}

/** Rough pill width at 13px: pills that do not fit the column give way to "+N". */
const CHAR_PX = 7
const PILL_CHROME_PX = 16

function OptionPills({ property, ids, width, wrap }: { property: Property; ids: string[]; width: number; wrap: boolean }) {
  let shown = ids.length
  if (!wrap) {
    const room = width - 16
    let used = 0
    shown = 0
    for (const id of ids) {
      const pill = Math.min(160, options(property).find((option) => option.id === id)?.name.length ?? 0) * CHAR_PX + PILL_CHROME_PX + 4
      const reserve = shown + 1 < ids.length ? 28 : 0
      if (shown > 0 && used + pill + reserve > room) break
      used += pill
      shown += 1
    }
  }
  return (
    <span className={cn('flex min-w-0 items-center gap-1', wrap ? 'flex-wrap py-1' : 'overflow-hidden')}>
      {ids.slice(0, shown).map((id) => (
        <OptionPill key={id} property={property} id={id} className={shown === 1 ? 'shrink' : undefined} />
      ))}
      {shown < ids.length ? <span className="shrink-0 text-xs text-muted-foreground">+{ids.length - shown}</span> : null}
    </span>
  )
}

function People({ ids, wrap, muted }: { ids: string[]; wrap: boolean; muted: boolean }) {
  const { users } = useDatabaseEnv()
  return (
    <span className={cn('flex min-w-0 items-center gap-2', wrap ? 'flex-wrap py-1' : 'overflow-hidden', muted && 'text-muted-foreground')}>
      {ids.map((id) => {
        const user = users.get(id)
        return (
          <span key={id} className="flex min-w-0 shrink-0 items-center gap-1.5 last:shrink">
            <UserAvatar user={user} name={user ? undefined : '?'} size={18} />
            <span className="truncate">{user?.name ?? 'Former member'}</span>
          </span>
        )
      })}
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
