import { useState, type DragEvent } from 'react'
import { Add, Gallery as GalleryIcon, Image } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import {
  DropdownMenuCheckboxItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from '@/components/ui/dropdown-menu'
import { coverObjectPosition, parseCoverPos } from '@/features/docs/coverLib'
import { CARD_SIZES, CARD_WIDTH, cardFace, galleryOf, type CardFace } from '@orbit/database/gallery'
import type { CardPreview, CardSize, Database, Property, Row, ViewConfig } from '@orbit/database/types'
import { cellOf } from '@orbit/database/values'
import { useMoveRow } from './api'
import { CellView } from './cells/CellView'
import { useDatabaseEnv } from './context'
import { PROPERTY_TYPES } from './propertyMeta'

interface DatabaseGalleryProps {
  database: Database
  config: ViewConfig
  /** The view's rows (filtered, searched, sorted, with new rows pinned). */
  rows: Row[]
  /** "+ New": a row opened in the peek. */
  onAddRow: () => void
  onOpenRow: (rowId: string) => void
  filtered: boolean
  onClearFilters: () => void
}

/** The preview box is 3:2 (spec P3 16.1). */
const PREVIEW_RATIO = 2 / 3

/**
 * The gallery view (spec P3 16.1): cards in a grid, each with a preview (the cover, the body's first image or text, or a
 * files property's first image), the title and the card properties; "+ New" is the last card. Without sorts, a card
 * drags to another place.
 */
export function DatabaseGallery({ database, config, rows, onAddRow, onOpenRow, filtered, onClearFilters }: DatabaseGalleryProps) {
  const { workspaceId, databaseId, canEditValues } = useDatabaseEnv()
  const moveRow = useMoveRow(workspaceId, databaseId)
  const [dragging, setDragging] = useState<string | null>(null)
  const [dropAt, setDropAt] = useState<number | null>(null)
  const gallery = galleryOf(config)
  const width = CARD_WIDTH[gallery.size]
  const title = database.properties.find((property) => property.type === 'title')
  const byId = new Map(database.properties.map((property) => [property.id, property]))
  const cardProperties = config.card_properties.flatMap((id) => byId.get(id) ?? [])
  const reorder = canEditValues && config.sorts.length === 0

  const drop = (index: number) => {
    const rowId = dragging
    setDragging(null)
    setDropAt(null)
    if (!rowId) return
    const from = rows.findIndex((row) => row.id === rowId)
    if (from === -1 || index === from || index === from + 1) return
    const rest = rows.filter((row) => row.id !== rowId)
    const at = index > from ? index - 1 : index
    moveRow.mutate({ rowId, afterRowId: at === 0 ? null : (rest[at - 1]?.id ?? null) })
  }

  return (
    <div className="flex flex-col gap-2 pb-4">
      {rows.length === 0 ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          {filtered && database.rows.length > 0 ? (
            <>
              No results
              <Button type="button" variant="outline" size="sm" onClick={onClearFilters}>
                Clear filters
              </Button>
            </>
          ) : (
            'No rows yet'
          )}
        </div>
      ) : null}
      <div role="list" aria-label="Cards" aria-busy={moveRow.isPending} className="grid gap-3" style={{ gridTemplateColumns: `repeat(auto-fill, minmax(min(${width}px, 100%), 1fr))` }}>
        {rows.map((row, index) => (
          <GalleryCard
            key={row.id}
            row={row}
            title={title}
            properties={cardProperties}
            face={cardFace(row, gallery, database.properties, workspaceId)}
            fitImage={gallery.fitImage}
            width={width}
            dragging={dragging === row.id}
            dropBefore={dropAt === index}
            draggable={reorder}
            onOpen={() => onOpenRow(row.id)}
            onDragStart={() => setDragging(row.id)}
            onDragEnd={() => {
              setDragging(null)
              setDropAt(null)
            }}
            onDragOver={(after) => {
              if (dragging) setDropAt(after ? index + 1 : index)
            }}
            onDrop={(after) => drop(after ? index + 1 : index)}
          />
        ))}
        {canEditValues ? (
          <button
            type="button"
            className="flex min-h-24 items-center justify-center gap-1.5 rounded-md border border-dashed border-border text-sm text-muted-foreground outline-none hover:bg-muted/40 focus-visible:ring-2 focus-visible:ring-ring/50"
            onClick={onAddRow}
          >
            <Add className="size-4" aria-hidden="true" />
            New
          </button>
        ) : null}
      </div>
    </div>
  )
}

interface GalleryCardProps {
  row: Row
  title: Property | undefined
  properties: Property[]
  face: CardFace | null
  fitImage: boolean
  width: number
  dragging: boolean
  /** The drop marker shows on this card's left edge. */
  dropBefore: boolean
  draggable: boolean
  onOpen: () => void
  onDragStart: () => void
  onDragEnd: () => void
  /** `after`: the pointer is on the card's right half. */
  onDragOver: (after: boolean) => void
  onDrop: (after: boolean) => void
}

function GalleryCard({ row, title, properties, face, fitImage, width, dragging, dropBefore, draggable, onOpen, onDragStart, onDragEnd, onDragOver, onDrop }: GalleryCardProps) {
  const filled = properties.filter((property) => cellOf(row, property) !== null)
  const after = (event: DragEvent<HTMLElement>) => {
    const rect = event.currentTarget.getBoundingClientRect()
    return event.clientX > rect.left + rect.width / 2
  }
  return (
    <article
      role="listitem"
      data-dragging={dragging || undefined}
      draggable={draggable}
      tabIndex={0}
      className={cn(
        'relative flex cursor-pointer flex-col overflow-hidden rounded-md border border-border bg-card text-sm outline-none hover:bg-muted/40 focus-visible:ring-2 focus-visible:ring-ring/50 data-dragging:opacity-35',
        dropBefore && 'before:absolute before:inset-y-0 before:-left-2 before:w-0.5 before:bg-primary',
      )}
      onClick={onOpen}
      onKeyDown={(event) => {
        if (event.target === event.currentTarget && event.key === 'Enter') onOpen()
      }}
      onDragStart={(event) => {
        event.dataTransfer.effectAllowed = 'move'
        event.dataTransfer.setData('text/plain', row.title)
        onDragStart()
      }}
      onDragEnd={onDragEnd}
      onDragOver={(event) => {
        if (!draggable) return
        event.preventDefault()
        onDragOver(after(event))
      }}
      onDrop={(event) => {
        event.preventDefault()
        onDrop(after(event))
      }}
    >
      {face ? <CardPreviewBox face={face} fitImage={fitImage} /> : null}
      <div className="flex min-w-0 flex-col gap-1.5 px-2.5 py-2">
        {title ? <CellView property={title} row={row} width={width - 20} wrap /> : null}
        {filled.map((property) => (
          <div key={property.id} className={cn('flex min-w-0 text-[13px]', property.type === 'checkbox' && '[&>span]:justify-start')}>
            <CellView property={property} row={row} width={width - 20} wrap={false} />
          </div>
        ))}
      </div>
    </article>
  )
}

function CardPreviewBox({ face, fitImage }: { face: CardFace; fitImage: boolean }) {
  return (
    <div className="relative w-full overflow-hidden border-b border-border bg-muted/40" style={{ paddingTop: `${PREVIEW_RATIO * 100}%` }}>
      {face.kind === 'image' ? (
        <img
          src={face.url}
          alt=""
          loading="lazy"
          draggable={false}
          className={cn('absolute inset-0 size-full', fitImage ? 'object-contain' : 'object-cover')}
          style={face.position ? { objectPosition: coverObjectPosition(parseCoverPos(face.position)) } : undefined}
        />
      ) : face.kind === 'text' ? (
        <p className="absolute inset-0 overflow-hidden px-3 pt-2.5 text-xs leading-relaxed whitespace-pre-line text-muted-foreground [mask-image:linear-gradient(to_bottom,black_70%,transparent)]">{face.text}</p>
      ) : null}
    </div>
  )
}

const PREVIEW_LABELS = { none: 'None', cover: 'Page cover', content: 'Page content' } as const
const SIZE_LABELS: Record<CardSize, string> = { small: 'Small', medium: 'Medium', large: 'Large' }

/** The gallery entries of the view options menu: card preview, card size and "Fit image". */
export function GalleryMenus({ properties, config, onConfig }: { properties: Property[]; config: ViewConfig; onConfig: (config: ViewConfig) => void }) {
  const gallery = galleryOf(config)
  const files = properties.filter((property) => property.type === 'files')
  const previewKey = (preview: CardPreview) => (typeof preview === 'object' ? `files:${preview.files}` : preview)
  const current = previewKey(gallery.preview)
  const preview = gallery.preview
  const currentLabel = typeof preview === 'object' ? (files.find((property) => property.id === preview.files)?.name ?? '') : PREVIEW_LABELS[preview]
  return (
    <>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger className="whitespace-nowrap">
          <Image />
          Card preview
          <span className="ml-auto max-w-20 min-w-0 truncate text-xs text-muted-foreground">{currentLabel}</span>
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent className="w-48">
          <DropdownMenuRadioGroup
            value={current}
            onValueChange={(key: string) => {
              const preview: CardPreview = key.startsWith('files:') ? { files: key.slice('files:'.length) } : (key as Exclude<CardPreview, object>)
              onConfig({ ...config, card_preview: preview })
            }}
          >
            {(['none', 'cover', 'content'] as const).map((preview) => (
              <DropdownMenuRadioItem key={preview} value={preview}>
                {PREVIEW_LABELS[preview]}
              </DropdownMenuRadioItem>
            ))}
            {files.map((property) => {
              const Icon = PROPERTY_TYPES[property.type].icon
              return (
                <DropdownMenuRadioItem key={property.id} value={previewKey({ files: property.id })}>
                  <Icon className="text-muted-foreground" />
                  <span className="truncate">{property.name}</span>
                </DropdownMenuRadioItem>
              )
            })}
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger className="whitespace-nowrap">
          <GalleryIcon />
          Card size
          <span className="ml-auto text-xs text-muted-foreground">{SIZE_LABELS[gallery.size]}</span>
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent className="w-40">
          <DropdownMenuRadioGroup value={gallery.size} onValueChange={(size: string) => onConfig({ ...config, card_size: size as CardSize })}>
            {CARD_SIZES.map((size) => (
              <DropdownMenuRadioItem key={size} value={size}>
                {SIZE_LABELS[size]}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuCheckboxItem checked={gallery.fitImage} disabled={gallery.preview === 'none'} onCheckedChange={(checked) => onConfig({ ...config, fit_image: checked })}>
        <Image />
        Fit image
      </DropdownMenuCheckboxItem>
    </>
  )
}
