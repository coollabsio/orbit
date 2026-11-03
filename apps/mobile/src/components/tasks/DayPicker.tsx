import DateTimePicker from '@react-native-community/datetimepicker'
import { Platform } from 'react-native'

import { useTheme } from '../../theme'

/** The system date picker: the inline calendar on iOS, the date dialog on Android. A pick calls `onPick` once. */
export function DayPicker({ value, onPick, onCancel }: { value: Date; onPick: (day: Date) => void; onCancel: () => void }) {
  const { colors, scheme } = useTheme()
  return (
    <DateTimePicker
      value={value}
      mode="date"
      display={Platform.OS === 'ios' ? 'inline' : 'default'}
      themeVariant={scheme}
      accentColor={colors.primary}
      onValueChange={(_event, day) => onPick(day)}
      onDismiss={onCancel}
    />
  )
}
