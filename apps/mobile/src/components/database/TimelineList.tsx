/**
 * A timeline view at phone size (spec P3 16.2): no time axis, its rows as a list grouped by the month their bar starts
 * in (rows without a date last, under "No date"), with "Timeline · Edit on the web".
 */
import { barOf, timelineOf } from '@orbit/database/timeline'
import type { Database, Row, ViewConfig, ViewContext } from '@orbit/database/types'
import { router } from 'expo-router'
import type { ReactElement } from 'react'
import { Pressable, SectionList, View } from 'react-native'

import { filledProperties } from '../../data/database'
import { Text } from '../../ui'
import { CellValue, RowTitle } from './cells'

type Section = { key: string; title: string; data: Row[] }

export function TimelineList({ database, config, rows, ctx, header, empty, refresh }: {
  database: Database
  config: ViewConfig
  /** The view's rows (filtered, searched and sorted). */
  rows: Row[]
  ctx: ViewContext
  header: ReactElement
  empty: ReactElement
  refresh: ReactElement
}) {
  const timeline = timelineOf(config, database.properties)
  const months = new Map<string, Row[]>()
  const undated: Row[] = []
  const spans = new Map<string, string>()
  const dayName = new Intl.DateTimeFormat(ctx.locale, { month: 'short', day: 'numeric', year: 'numeric', timeZone: 'UTC' })
  const day = (iso: string) => dayName.format(new Date(`${iso}T00:00:00Z`))
  for (const row of rows) {
    const bar = barOf(row, timeline, ctx.timeZone)
    if (!bar) {
      undated.push(row)
      continue
    }
    spans.set(row.id, bar.startDay === bar.endDay ? day(bar.startDay) : `${day(bar.startDay)} – ${day(bar.endDay)}`)
    const month = bar.startDay.slice(0, 7)
    months.set(month, [...(months.get(month) ?? []), row])
  }
  const monthName = new Intl.DateTimeFormat(ctx.locale, { month: 'long', year: 'numeric', timeZone: 'UTC' })
  const sections: Section[] = [...months.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([month, data]) => ({ key: month, title: monthName.format(new Date(`${month}-01T00:00:00Z`)), data }))
  if (undated.length > 0) sections.push({ key: 'none', title: 'No date', data: undated })
  const columns = timeline.columns

  return (
    <SectionList
      sections={sections}
      keyExtractor={(row) => row.id}
      ListHeaderComponent={
        <>
          {header}
          <Text className="mx-3 mt-2 px-1.5 text-[12px] text-muted-foreground">Timeline · Edit on the web</Text>
        </>
      }
      ListEmptyComponent={empty}
      keyboardShouldPersistTaps="handled"
      contentContainerClassName="pb-8"
      stickySectionHeadersEnabled={false}
      refreshControl={refresh}
      renderSectionHeader={({ section }) => <Text className="mx-3 mt-4 px-1.5 pb-1 font-noto-medium text-[12px] text-muted-foreground">{section.title}</Text>}
      renderItem={({ item }) => {
        const span = spans.get(item.id)
        const shown = filledProperties(item, columns)
        return (
          <Pressable accessibilityRole="link" accessibilityLabel={item.title || 'Untitled'} onPress={() => router.push(`/docs/${item.id}`)} className="mx-3 py-2.5 px-1.5 gap-1.5 border-b border-border active:bg-muted">
            <RowTitle row={item} />
            {span || shown.length > 0 ? (
              <View className="flex-row flex-wrap items-center gap-1.5">
                {span ? <Text className="text-[12px] text-muted-foreground">{span}</Text> : null}
                {shown.map((property) => (
                  <CellValue key={property.id} property={property} row={item} ctx={ctx} chip />
                ))}
              </View>
            ) : null}
          </Pressable>
        )
      }}
    />
  )
}
