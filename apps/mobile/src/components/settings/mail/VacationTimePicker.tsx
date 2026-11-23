import DateTimePicker, { DateTimePickerAndroid } from '@react-native-community/datetimepicker'
import { useEffect } from 'react'
import { Platform } from 'react-native'

import { useTheme } from '../../../theme'

/**
 * The system date and time picker: inline on iOS (each change calls `onPick`; the field's Done closes it), the date
 * dialog then the time dialog on Android (a pick calls `onPick`, then `onClose`).
 */
export function VacationTimePicker({ value, minimumDate, onPick, onClose }: { value: Date; minimumDate?: Date; onPick: (at: Date) => void; onClose: () => void }) {
  const { colors, scheme } = useTheme()
  useEffect(() => {
    if (Platform.OS !== 'android') return
    DateTimePickerAndroid.open({
      value,
      mode: 'date',
      minimumDate,
      onValueChange: (_event, day) =>
        DateTimePickerAndroid.open({
          value: day,
          mode: 'time',
          is24Hour: true,
          onValueChange: (_timeEvent, at) => {
            onPick(at)
            onClose()
          },
          onDismiss: onClose,
        }),
      onDismiss: onClose,
    })
    // opened once, when the field asks for it
  }, [])
  if (Platform.OS === 'android') return null
  return <DateTimePicker value={value} mode="datetime" display="inline" minimumDate={minimumDate} themeVariant={scheme} accentColor={colors.primary} onValueChange={(_event, at) => onPick(at)} />
}
