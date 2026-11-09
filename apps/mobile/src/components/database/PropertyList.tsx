import { isComputed } from '@orbit/database/meta'
import type { CellValue, Property, Row, SelectOption } from '@orbit/database/types'
import { apiValue, cellOf } from '@orbit/database/values'
import { repairConfig } from '@orbit/database/view'
import { useState } from 'react'
import { Pressable, View } from 'react-native'

import { PROPERTY_ICONS, pickView } from '../../data/database'
import { type Notify, useDatabase, useSaveOptions, useUpdateValues, useViewContext, useWorkspaceReadOnly } from '../../hooks/database'
import { useTheme } from '../../theme'
import { Icon, Text } from '../../ui'
import { Sheet } from '../tasks/Sheet'
import { CellValue as ValueView } from './cells'
import { ValueEditor } from './ValueEditor'

/**
 * The properties of a row page under its title (spec 7.5 and 8): every property but the title, hidden ones included,
 * in the column order of the database's first view. A tap on a value opens its sheet editor; a checkbox toggles.
 */
export function PropertyList({ databaseId, rowId, notify }: { databaseId: string; rowId: string; notify: Notify }) {
  const query = useDatabase(databaseId)
  const ctx = useViewContext()
  const readOnly = useWorkspaceReadOnly()
  const updateValues = useUpdateValues(databaseId, notify)
  const saveOptions = useSaveOptions(databaseId, notify)
  const [editing, setEditing] = useState<string | null>(null)
  const [visible, setVisible] = useState(false)
  const database = query.data
  const row = database?.rows.find((candidate) => candidate.id === rowId)
  if (!database || !row) return null

  const view = pickView(database.views, null)
  const byId = new Map(database.properties.map((property) => [property.id, property]))
  const order = view ? repairConfig(view, database.properties).columns.map((column) => column.property_id) : database.properties.map((property) => property.id)
  const properties = order.flatMap((id) => {
    const property = byId.get(id)
    return property && property.type !== 'title' ? [property] : []
  })
  const canEditSchema = !database.database.locked && !readOnly
  const commit = (property: Property) => (next: CellValue | null) => updateValues.mutate({ rowId: row.id, values: { [property.id]: apiValue(next) } })
  const onOptions = (property: Property) => async (options: SelectOption[]) => {
    await saveOptions.mutateAsync({ propertyId: property.id, config: property.config, options })
  }
  const edited = editing ? byId.get(editing) : undefined

  return (
    <View accessibilityLabel="Properties" className="mx-3 mb-4 pb-3 border-b border-border">
      {properties.map((property) => (
        <PropertyLine
          key={property.id}
          property={property}
          row={row}
          editable={!readOnly && !isComputed(property.type)}
          onEdit={() => {
            setEditing(property.id)
            setVisible(true)
          }}
          onToggle={() => commit(property)(cellOf(row, property) ? null : { kind: 'checked' })}
          ctx={ctx}
        />
      ))}
      {canEditSchema ? <Text className="text-[12px] px-1.5 pt-1.5 text-muted-foreground/70">Add and change properties on the web.</Text> : null}
      <Sheet visible={visible} title={edited?.name ?? ''} onClose={() => setVisible(false)}>
        {edited ? (
          <ValueEditor
            key={`${row.id}:${edited.id}`}
            property={edited}
            row={row}
            onCommit={commit(edited)}
            onOptions={canEditSchema ? onOptions(edited) : undefined}
            onDone={() => setVisible(false)}
          />
        ) : null}
      </Sheet>
    </View>
  )
}

function PropertyLine({ property, row, editable, onEdit, onToggle, ctx }: {
  property: Property
  row: Row
  editable: boolean
  onEdit: () => void
  onToggle: () => void
  ctx: ReturnType<typeof useViewContext>
}) {
  const { colors } = useTheme()
  const value = cellOf(row, property)
  const checkbox = property.type === 'checkbox'
  return (
    <Pressable
      accessibilityRole={editable ? 'button' : undefined}
      accessibilityLabel={editable ? `Edit ${property.name}` : property.name}
      disabled={!editable}
      onPress={checkbox ? onToggle : onEdit}
      className="min-h-[38px] flex-row items-start gap-1 rounded-md active:bg-muted"
    >
      <View className="h-[38px] w-[38%] flex-row items-center gap-1.5 px-1.5">
        <Icon name={PROPERTY_ICONS[property.type]} size={15} color={colors.mutedForeground} />
        <Text numberOfLines={1} className="text-[14px] text-muted-foreground shrink">
          {property.name}
        </Text>
      </View>
      <View className="min-h-[38px] flex-1 justify-center items-start py-1.5 px-1.5">
        {checkbox ? (
          <Icon name={value ? 'checkSquare' : 'square'} size={18} color={value ? colors.primary : colors.mutedForeground} />
        ) : value === null && !isComputed(property.type) ? (
          <Text className="text-[14px] text-muted-foreground/70">Empty</Text>
        ) : (
          <ValueView property={property} row={row} ctx={ctx} />
        )}
      </View>
    </Pressable>
  )
}
