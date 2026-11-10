/** Relation and rollup values at phone size (spec P3 11.8): related pages as pills that open the row, a read-only rollup, and the search sheet that links pages. */
import { hiddenLinks, MAX_RELATION_LINKS, relatedTitle, rollupHidesPages, type RelationProperty, type RollupProperty } from '@orbit/database/relations'
import type { CellValue, DatabaseRelatedPage, Row, ViewContext } from '@orbit/database/types'
import { cellOf, displayText } from '@orbit/database/values'
import { router } from 'expo-router'
import { useState } from 'react'
import { Pressable, View } from 'react-native'

import { useDatabase, useRememberRelatedPages } from '../../hooks/database'
import { Text } from '../../ui'
import { SheetList, SheetNote, SheetRow, SheetSearch } from '../tasks/Sheet'
import { Pill } from './cells'

/** Rows the sheet lists under the linked ones at once; typing narrows them. */
const PICKER_ROWS = 50

/** A relation value: a pill per related page (a tap opens it), then "+N without access". */
export function RelationValue({ property, row, ctx }: { property: RelationProperty; row: Row; ctx: ViewContext }) {
  const value = cellOf(row, property)
  const ids = value?.kind === 'pages' ? value.ids : []
  const hidden = hiddenLinks(row, property.id)
  if (ids.length === 0 && hidden === 0) return null
  return (
    <View className="flex-row flex-wrap items-center gap-1 min-w-0">
      {ids.map((id) => {
        const title = relatedTitle(ctx, id) ?? 'Untitled'
        const icon = ctx.relatedPages?.[id]?.icon
        return (
          <Pressable key={id} accessibilityRole="link" accessibilityLabel={`Open ${title}`} hitSlop={4} onPress={() => router.push(`/docs/${id}`)} className="max-w-full active:opacity-60">
            <Pill color="default" label={icon ? `${icon} ${title}` : title} />
          </Pressable>
        )
      })}
      {hidden > 0 ? <Text className="text-[12px] text-muted-foreground">+{hidden} without access</Text> : null}
    </View>
  )
}

/** A rollup value, read-only: a list as pills, a number or a date as text; a note when pages the viewer cannot read are left out. */
export function RollupValue({ property, row, ctx, chip }: { property: RollupProperty; row: Row; ctx: ViewContext; chip: boolean }) {
  const value = cellOf(row, property)
  const hiddenNote = !chip && rollupHidesPages(row, property) ? <Text className="text-[12px] text-muted-foreground">Some pages are hidden</Text> : null
  if (!value) return hiddenNote
  const content =
    value.kind === 'list' ? (
      <View className="flex-row flex-wrap gap-1 min-w-0">
        {value.items.map((item, index) => (
          <Pill key={index} color="default" label={item} />
        ))}
      </View>
    ) : chip ? (
      <View className="h-5 max-w-full justify-center px-1.5 rounded-[4px] border border-border">
        <Text numberOfLines={1} className="text-[12px] leading-4 text-muted-foreground">
          {displayText(property, value, ctx)}
        </Text>
      </View>
    ) : (
      <Text className="text-[14px] text-muted-foreground">{displayText(property, value, ctx)}</Text>
    )
  return hiddenNote ? (
    <View className="gap-0.5">
      {content}
      {hiddenNote}
    </View>
  ) : (
    content
  )
}

/** The relation sheet: the linked pages, then the target database's rows by title. Each tap saves; a one-page relation replaces its page and closes. */
export function RelationEditor({ property, row, onCommit, onDone }: { property: RelationProperty; row: Row; onCommit: (value: CellValue | null) => void; onDone: () => void }) {
  const target = useDatabase(property.config.database_id)
  const remember = useRememberRelatedPages()
  const value = cellOf(row, property)
  const [ids, setIds] = useState(value?.kind === 'pages' ? value.ids : [])
  const [query, setQuery] = useState('')
  const [notice, setNotice] = useState<string | null>(null)
  const one = property.config.limit === 'one'
  const rows = target.data?.rows ?? []
  const byId = new Map(rows.map((candidate) => [candidate.id, candidate]))
  const titleOf = (id: string) => {
    const known = byId.get(id)
    return known ? known.title || 'Untitled' : 'Untitled'
  }
  const needle = query.trim().toLowerCase()
  const fits = (id: string) => needle === '' || titleOf(id).toLowerCase().includes(needle)
  const linked = ids.filter(fits)
  const candidates = rows.filter((candidate) => !ids.includes(candidate.id) && fits(candidate.id)).slice(0, PICKER_ROWS)
  const hidden = hiddenLinks(row, property.id)

  const commit = (next: string[]) => {
    const pages: Record<string, DatabaseRelatedPage> = {}
    for (const id of next) {
      const known = byId.get(id)
      if (known) pages[id] = { title: known.title, icon: known.icon, database_id: property.config.database_id }
    }
    remember(row.id, pages)
    setIds(next)
    onCommit(next.length > 0 ? { kind: 'pages', ids: next } : null)
  }
  const toggle = (id: string) => {
    setNotice(null)
    if (one) {
      commit(ids[0] === id ? [] : [id])
      onDone()
    } else if (ids.includes(id)) commit(ids.filter((other) => other !== id))
    else if (ids.length >= MAX_RELATION_LINKS) setNotice(`A relation links at most ${MAX_RELATION_LINKS} pages.`)
    else commit([...ids, id])
  }
  const rowLabel = (icon: string | null | undefined, title: string) => (icon ? `${icon} ${title}` : title)

  return (
    <>
      <SheetSearch value={query} onChange={setQuery} placeholder={`Search ${target.data?.database.title || 'pages'}…`} />
      <SheetList>
        {linked.map((id) => (
          <SheetRow key={id} label={rowLabel(byId.get(id)?.icon, titleOf(id))} checked onPress={() => toggle(id)} />
        ))}
        {candidates.map((candidate) => (
          <SheetRow key={candidate.id} label={rowLabel(candidate.icon, candidate.title || 'Untitled')} onPress={() => toggle(candidate.id)} />
        ))}
        {target.isError ? <SheetNote tone="destructive">You cannot open the related database.</SheetNote> : null}
        {target.isPending ? <SheetNote>Loading…</SheetNote> : null}
        {target.data && linked.length === 0 && candidates.length === 0 ? <SheetNote>No pages</SheetNote> : null}
        {notice ? <SheetNote tone="destructive">{notice}</SheetNote> : null}
        {hidden > 0 ? <SheetNote>{hidden === 1 ? '1 linked page you cannot open stays linked.' : `${hidden} linked pages you cannot open stay linked.`}</SheetNote> : null}
      </SheetList>
    </>
  )
}
