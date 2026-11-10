/** A formula value at phone size: the server's result in its type's look, or a muted "Error" whose message a tap shows (spec P3 15.7). */
import { formulaBase, plainFormulaMessage, type FormulaProperty } from '@orbit/database/formula/values'
import type { Row, ViewContext } from '@orbit/database/types'
import { Alert, Pressable } from 'react-native'

import { useTheme } from '../../theme'
import { Icon, Text } from '../../ui'
import { CellValue } from './cells'

export function FormulaValue({ property, row, ctx, chip }: { property: FormulaProperty; row: Row; ctx: ViewContext; chip: boolean }) {
  const { colors } = useTheme()
  const error = row.formulaErrors?.[property.id]
  if (error === undefined) return <CellValue property={formulaBase(property)} row={row} ctx={ctx} chip={chip} />
  const message = plainFormulaMessage(error)
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${property.name}: error. ${message}`}
      hitSlop={8}
      onPress={() => Alert.alert(property.name, message)}
      className="flex-row items-center gap-1"
    >
      <Icon name="warning" size={13} color={colors.mutedForeground} />
      <Text className="text-[13px] text-muted-foreground">Error</Text>
    </Pressable>
  )
}
