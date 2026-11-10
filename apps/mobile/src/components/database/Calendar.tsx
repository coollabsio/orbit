/**
 * A calendar view at phone size (spec P3 13.2): a month grid with a dot per row on each day, and the rows of the
 * tapped day under it. Dates change in a row's property list; dragging is web only.
 */
import { barsOn, calendarWeeks, datable, dayDate, localeWeekStart, monthTitle, placeRows, rowSpan, startTime, undatedRows, weekdayName } from '@orbit/database/calendar'
import { addMonths, localDay } from '@orbit/database/dates'
import type { Database, DateProperty, Property, Row, ViewConfig, ViewContext } from '@orbit/database/types'
import { apiValue } from '@orbit/database/values'
import { router } from 'expo-router'
import { type ReactElement, useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import { filledProperties } from '../../data/database'
import { Button, IconButton, Text } from '../../ui'
import { CellValue, RowTitle } from './cells'

/** Dots a day shows at most; more rows still show in the day list. */
const MAX_DOTS = 3

export function Calendar({ database, config, rows, ctx, header, refresh, canAdd, onAdd }: {
  database: Database
  config: ViewConfig
  /** The view's rows (filtered, searched and sorted). */
  rows: Row[]
  ctx: ViewContext
  header: ReactElement
  refresh: ReactElement
  canAdd: boolean
  /** A new row with `values` (the tapped day's date). */
  onAdd: (values: Record<string, unknown>) => void
}) {
  const today = localDay(ctx.now, ctx.timeZone)
  const [anchor, setAnchor] = useState(today)
  const [selected, setSelected] = useState(today)
  const [showUndated, setShowUndated] = useState(false)
  const candidate = database.properties.find((property) => property.id === config.date_property_id)

  if (!datable(candidate)) {
    return (
      <ScrollView contentContainerClassName="pb-8" keyboardShouldPersistTaps="handled" refreshControl={refresh}>
        {header}
        <View className="items-center gap-1 py-10 px-6">
          <Text className="text-[14px] text-muted-foreground">Choose a date property</Text>
          <Text className="text-[12px] text-muted-foreground text-center">A calendar places its rows by a Date, Created time or Last edited time property. Edit on the web.</Text>
        </View>
      </ScrollView>
    )
  }
  const property = candidate
  const weeks = calendarWeeks(anchor, localeWeekStart(ctx.locale))
  const month = anchor.slice(0, 7)
  const dayRows = rows.filter((row) => {
    const span = rowSpan(row, property, ctx.timeZone)
    return span !== null && span.start <= selected && span.end >= selected
  })
  const undated = undatedRows(rows, property, ctx.timeZone)
  const cardProperties = config.card_properties.flatMap((id) => database.properties.find((other) => other.id === id) ?? [])
  const step = (months: number) => setAnchor((day) => addMonths(day, months))

  return (
    <ScrollView contentContainerClassName="pb-8" keyboardShouldPersistTaps="handled" refreshControl={refresh}>
      {header}
      <View className="mx-3 mt-2 flex-row items-center gap-1">
        <Text className="font-noto-semibold text-[16px] text-foreground flex-1">{monthTitle(anchor, ctx.locale)}</Text>
        <IconButton icon="chevronLeft" label="Previous month" onPress={() => step(-1)} />
        <Button
          label="Today"
          variant="ghost"
          size="sm"
          onPress={() => {
            setAnchor(today)
            setSelected(today)
          }}
        />
        <IconButton icon="chevronRight" label="Next month" onPress={() => step(1)} />
      </View>
      <View className="mx-3 mt-1">
        <View className="flex-row">
          {weeks[0]!.map((day) => (
            <Text key={day} className="flex-1 text-center text-[11px] text-muted-foreground py-1">
              {weekdayName(day, ctx.locale)}
            </Text>
          ))}
        </View>
        {weeks.map((days) => {
          const bars = placeRows(rows, property, days, ctx.timeZone)
          return (
            <View key={days[0]} className="flex-row">
              {days.map((day, column) => {
                const count = barsOn(bars, column).length
                const isSelected = day === selected
                return (
                  <Pressable
                    key={day}
                    accessibilityRole="button"
                    accessibilityLabel={`${day}, ${count === 1 ? '1 row' : `${count} rows`}`}
                    accessibilityState={{ selected: isSelected }}
                    onPress={() => setSelected(day)}
                    className="flex-1 items-center py-1.5 gap-1 rounded-md active:bg-muted"
                  >
                    <View className={`h-7 w-7 items-center justify-center rounded-full ${isSelected ? 'bg-primary' : day === today ? 'bg-primary/15' : ''}`}>
                      <Text className={`text-[13px] ${isSelected ? 'text-primary-foreground' : day.slice(0, 7) === month ? 'text-foreground' : 'text-muted-foreground'}`}>{Number(day.slice(8))}</Text>
                    </View>
                    <View className="h-1.5 flex-row gap-0.5">
                      {Array.from({ length: Math.min(count, MAX_DOTS) }, (_, index) => (
                        <View key={index} className="h-1.5 w-1.5 rounded-full bg-primary" />
                      ))}
                    </View>
                  </Pressable>
                )
              })}
            </View>
          )
        })}
      </View>
      <View className="mx-3 mt-3 flex-row items-center border-b border-border pb-1.5 px-1.5">
        <Text className="font-noto-medium text-[13px] text-muted-foreground flex-1">
          {new Intl.DateTimeFormat(ctx.locale, { timeZone: 'UTC', weekday: 'long', month: 'long', day: 'numeric' }).format(Date.parse(`${selected}T00:00:00Z`))}
        </Text>
        {canAdd && property.type === 'date' ? (
          <IconButton icon="plus" label={`New row on ${selected}`} onPress={() => onAdd({ [property.id]: apiValue({ kind: 'date', date: dayDate(selected) }) })} />
        ) : null}
      </View>
      {dayRows.length === 0 ? <Text className="mx-3 py-4 px-1.5 text-[13px] text-muted-foreground">No rows on this day</Text> : null}
      {dayRows.map((row) => (
        <DayRow key={row.id} row={row} property={property} properties={filledProperties(row, cardProperties)} ctx={ctx} />
      ))}
      {undated.length > 0 ? (
        <View className="mx-3 mt-3">
          <Button label={`No date (${undated.length})`} variant="ghost" size="sm" muted onPress={() => setShowUndated((shown) => !shown)} />
          {showUndated ? undated.map((row) => <DayRow key={row.id} row={row} property={property} properties={filledProperties(row, cardProperties)} ctx={ctx} />) : null}
        </View>
      ) : null}
    </ScrollView>
  )
}

function DayRow({ row, property, properties, ctx }: { row: Row; property: DateProperty; properties: Property[]; ctx: ViewContext }) {
  const time = startTime(row, property, ctx)
  return (
    <Pressable accessibilityRole="link" accessibilityLabel={row.title || 'Untitled'} onPress={() => router.push(`/docs/${row.id}`)} className="mx-3 py-2.5 px-1.5 gap-1.5 border-b border-border active:bg-muted">
      <View className="flex-row items-center gap-2">
        {time ? <Text className="text-[12px] text-muted-foreground">{time}</Text> : null}
        <View className="flex-1 min-w-0">
          <RowTitle row={row} />
        </View>
      </View>
      {properties.length > 0 ? (
        <View className="flex-row flex-wrap items-center gap-1.5">
          {properties.map((other) => (
            <CellValue key={other.id} property={other} row={row} ctx={ctx} chip />
          ))}
        </View>
      ) : null}
    </Pressable>
  )
}
