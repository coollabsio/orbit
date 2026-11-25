import type { TaskField } from '@orbit/api/generated/types.gen'
import { useEffect, useState } from 'react'
import { Pressable, View } from 'react-native'

import { FIELD_KIND_ICON, orderKey, orderOptions, orderPatch, toggleCustomField } from '../../data/task-display'
import type { DisplayOptions } from '../../data/tasks-filter'
import { useOrbit } from '../../orbit-context'
import { useTheme } from '../../theme'
import { Icon, Text } from '../../ui'
import { Segmented } from '../settings/Shell'
import { Sheet, SheetList, SheetNote, SheetRow } from './Sheet'

type Page = 'main' | 'order'

/**
 * The task list's display options, as the web's Display popover at phone size: grouping, ordering and its direction,
 * and the custom fields rows show. Each change goes to `onChange` at once (the page preference saves it).
 */
export function DisplaySheet({
  visible,
  display,
  fields,
  onChange,
  onClose,
}: {
  visible: boolean
  display: DisplayOptions
  /** The fields to offer (`displayFields`). */
  fields: readonly TaskField[]
  onChange: (patch: Partial<DisplayOptions>) => void
  onClose: () => void
}) {
  const { colors } = useTheme()
  const { workspace } = useOrbit()
  const [page, setPage] = useState<Page>('main')
  useEffect(() => {
    if (visible) setPage('main')
  }, [visible])
  const currentOrder = orderKey(display)
  const orders = orderOptions(fields, currentOrder, workspace)
  const manual = currentOrder === 'manual'
  const shown = display.custom_fields ?? []

  if (page === 'order') {
    return (
      <Sheet visible={visible} title="Ordering" onClose={onClose}>
        <SheetList>
          {orders.map((option) => (
            <SheetRow
              key={option.key}
              label={option.label}
              checked={option.key === currentOrder}
              onPress={() => {
                onChange(orderPatch(option.key))
                setPage('main')
              }}
            />
          ))}
        </SheetList>
      </Sheet>
    )
  }

  return (
    <Sheet visible={visible} title="Display" onClose={onClose}>
      <SheetList>
        <View className="px-4 py-2 gap-2">
          <Text className="font-noto-medium text-[14px] leading-[19px]">Grouping</Text>
          <Segmented
            label="Grouping"
            value={display.group_by === 'none' ? 'none' : 'status'}
            options={[
              { value: 'status', label: 'Status' },
              { value: 'none', label: 'No grouping' },
            ]}
            onChange={(value) => onChange({ group_by: value })}
          />
        </View>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Ordering: ${orders.find((option) => option.key === currentOrder)?.label}`}
          onPress={() => setPage('order')}
          className="flex-row items-center gap-2 min-h-11 px-4 active:bg-accent"
        >
          <Text className="flex-1 font-noto-medium text-[14px] leading-[19px]">Ordering</Text>
          <Text numberOfLines={1} className="shrink text-[14px] leading-[19px] text-muted-foreground">
            {orders.find((option) => option.key === currentOrder)?.label}
          </Text>
          <Icon name="chevronRight" size={16} color={colors.mutedForeground} />
        </Pressable>
        <View className="px-4 pb-2">
          <Segmented
            label="Direction"
            disabled={manual}
            value={display.order_direction}
            options={[
              { value: 'asc', label: 'Ascending' },
              { value: 'desc', label: 'Descending' },
            ]}
            onChange={(value) => onChange({ order_direction: value })}
          />
          {manual ? <Text className="pt-1 text-[12px] leading-[16px] text-muted-foreground">Manual order has no direction.</Text> : null}
        </View>
        <Text className="px-4 pt-3 pb-1 font-noto-medium text-[14px] leading-[19px]">Custom fields</Text>
        {fields.length === 0 ? <SheetNote>No custom fields here yet. Add them in Settings → Task fields.</SheetNote> : null}
        {fields.map((field) => (
          <SheetRow key={field.id} label={field.name} icon={FIELD_KIND_ICON[field.kind]} checked={shown.includes(field.id)} onPress={() => onChange(toggleCustomField(display, field.id, fields))} />
        ))}
      </SheetList>
    </Sheet>
  )
}
