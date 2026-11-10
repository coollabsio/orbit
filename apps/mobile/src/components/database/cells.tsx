/** What a database value looks like at phone size: the web's `CellView`, as option pills, people and quiet text. */
import { isComputed } from '@orbit/database/meta'
import type { OptionColor, Property, Row, ViewContext } from '@orbit/database/types'
import { cellOf, displayText, options } from '@orbit/database/values'
import { type ReactNode } from 'react'
import { Linking, Pressable, View } from 'react-native'

import { PILL_COLORS } from '../../data/database'
import { useTheme } from '../../theme'
import { Avatar, Icon, Text } from '../../ui'
import { ButtonCell } from './ButtonCell'
import { FormulaValue } from './FormulaValue'
import { RelationValue, RollupValue } from './Relation'
import { contactHref } from '@orbit/database/fields'

/** A select, multi-select or status option; a status pill is round with a dot of its color. */
export function Pill({ color, label, dot = false }: { color: OptionColor; label: string; dot?: boolean }) {
  const { scheme } = useTheme()
  const look = PILL_COLORS[scheme][color]
  return (
    <View className={`h-5 max-w-full flex-row items-center gap-1.5 px-1.5 ${dot ? 'rounded-full' : 'rounded-[4px]'} ${look.box}`}>
      {dot ? <View className={`size-2 rounded-full opacity-70 ${look.dot}`} /> : null}
      <Text numberOfLines={1} className={`shrink text-[13px] leading-5 ${look.text}`}>
        {label}
      </Text>
    </View>
  )
}

export function OptionPill({ property, id }: { property: Property; id: string }) {
  const option = options(property).find((candidate) => candidate.id === id)
  return option ? <Pill color={option.color} label={option.name} dot={property.type === 'status'} /> : null
}

/** A sheet row whose label is a pill or an avatar line (the `SheetRow` look, without a second name). */
export function ChoiceRow({ label, checked, onPress, children }: { label: string; checked: boolean; onPress: () => void; children: ReactNode }) {
  const { colors } = useTheme()
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={label} accessibilityState={{ selected: checked }} onPress={onPress} className="min-h-11 flex-row items-center gap-3 px-4 active:bg-accent">
      <View className="flex-1 flex-row items-center gap-2 min-w-0">{children}</View>
      {checked ? <Icon name="check" size={18} color={colors.primary} /> : null}
    </Pressable>
  )
}

/** The row title with its icon and the "has text" mark, as the title cell. */
export function RowTitle({ row, className = 'text-[14px]' }: { row: Row; className?: string }) {
  const { colors } = useTheme()
  return (
    <View className="flex-row items-center gap-1.5 min-w-0">
      {row.icon ? <Text className="text-[14px]">{row.icon}</Text> : null}
      <Text numberOfLines={2} className={`font-noto-medium shrink ${className} ${row.title ? 'text-foreground' : 'text-muted-foreground'}`}>
        {row.title || 'Untitled'}
      </Text>
      {row.has_content ? (
        <View accessibilityLabel="Has text">
          <Icon name="docs" size={13} color={colors.mutedForeground} />
        </View>
      ) : null}
    </View>
  )
}

/**
 * A value of a row (never the title). `chip` frames plain text in a small border, as the row list shows its properties;
 * a checkbox chip names its property, since a lone check says nothing.
 */
export function CellValue({ property, row, ctx, chip = false }: { property: Property; row: Row; ctx: ViewContext; chip?: boolean }) {
  const { colors } = useTheme()
  if (property.type === 'button') return <ButtonCell property={property} row={row} />
  if (property.type === 'formula') return <FormulaValue property={property} row={row} ctx={ctx} chip={chip} />
  if (property.type === 'relation') return <RelationValue property={property} row={row} ctx={ctx} />
  if (property.type === 'rollup') return <RollupValue property={property} row={row} ctx={ctx} chip={chip} />
  const value = cellOf(row, property)
  if (!value) return null
  switch (property.type) {
    case 'select':
    case 'status':
      return value.kind === 'choice' ? <OptionPill property={property} id={value.id} /> : null
    case 'multi_select':
      return value.kind === 'choices' ? (
        <View className="flex-row flex-wrap gap-1 min-w-0">
          {value.ids.map((id) => (
            <OptionPill key={id} property={property} id={id} />
          ))}
        </View>
      ) : null
    case 'person':
    case 'created_by':
    case 'last_edited_by':
      return value.kind === 'people' ? (
        <View className="flex-row flex-wrap gap-x-2 gap-y-1 min-w-0">
          {value.ids.map((id) => {
            const name = ctx.members.get(id)
            return (
              <View key={id} className="flex-row items-center gap-1.5">
                <Avatar id={id} name={name ?? '?'} size={18} />
                <Text numberOfLines={1} className={`text-[13px] ${isComputed(property.type) ? 'text-muted-foreground' : 'text-foreground'}`}>
                  {name ?? 'Former member'}
                </Text>
              </View>
            )
          })}
        </View>
      ) : null
    case 'checkbox':
      return (
        <View className={`flex-row items-center gap-1 ${chip ? 'h-5 px-1.5 rounded-[4px] border border-border' : ''}`}>
          <Icon name="checkSquare" size={chip ? 13 : 18} color={colors.primary} />
          {chip ? <Text className="text-[12px] text-muted-foreground">{property.name}</Text> : null}
        </View>
      )
    case 'url':
      return value.kind === 'url' ? (
        <Text numberOfLines={1} onPress={() => void Linking.openURL(value.url).catch(() => {})} className={`text-[13px] text-foreground underline ${chip ? 'px-1.5 rounded-[4px] border border-border' : ''}`}>
          {urlLabel(value.url)}
        </Text>
      ) : null
    case 'email':
    case 'phone':
      return value.kind === 'text' ? (
        <Text numberOfLines={1} onPress={() => void Linking.openURL(contactHref(property.type === 'email' ? 'email' : 'phone', value.text)).catch(() => {})} className={`text-[13px] text-foreground underline ${chip ? 'px-1.5 rounded-[4px] border border-border' : ''}`}>
          {value.text}
        </Text>
      ) : null
    case 'files':
      return value.kind === 'files' ? (
        <View className="flex-row flex-wrap gap-1 min-w-0">
          {value.files.map((file) => (
            <View key={file.file_id} className="h-5 max-w-full flex-row items-center gap-1 px-1.5 rounded-[4px] bg-muted">
              <Icon name="attach" size={12} color={colors.mutedForeground} />
              <Text numberOfLines={1} className="shrink text-[12px] leading-4 text-foreground">
                {file.name}
              </Text>
            </View>
          ))}
        </View>
      ) : null
    case 'title':
      return null
    case 'unique_id':
    case 'text':
    case 'number':
    case 'date':
    case 'created_time':
    case 'last_edited_time': {
      const muted = property.type === 'created_time' || property.type === 'last_edited_time' || property.type === 'unique_id'
      return chip ? (
        <View className="h-5 max-w-full justify-center px-1.5 rounded-[4px] border border-border">
          <Text numberOfLines={1} className="text-[12px] leading-4 text-muted-foreground">
            {displayText(property, value, ctx)}
          </Text>
        </View>
      ) : (
        <Text className={`text-[14px] ${muted ? 'text-muted-foreground' : 'text-foreground'}`}>{displayText(property, value, ctx)}</Text>
      )
    }
  }
}

/** `https://example.com/a` as `example.com/a`, as the web shows a link. */
function urlLabel(url: string): string {
  const match = /^https?:\/\/([^/?#]+)([^?#]*)/i.exec(url)
  if (!match) return url
  return `${match[1]}${match[2] === '/' ? '' : match[2]}`
}
