import { createElement } from 'react'

import { fonts, useTheme } from '../../../theme'

const pad = (value: number) => String(value).padStart(2, '0')
const inputValue = (at: Date) => `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(at.getHours())}:${pad(at.getMinutes())}`

/** The browser's date and time field: the system picker has no web version (the web preview of the app). */
export function VacationTimePicker({ value, minimumDate, onPick }: { value: Date; minimumDate?: Date; onPick: (at: Date) => void; onClose: () => void }) {
  const { colors, scheme } = useTheme()
  return createElement('input', {
    type: 'datetime-local',
    defaultValue: inputValue(value),
    min: minimumDate ? inputValue(minimumDate) : undefined,
    onChange: (event: { target: { value: string } }) => {
      const [day, time] = event.target.value.split('T')
      const [year, month, date] = (day ?? '').split('-').map(Number)
      const [hours, minutes] = (time ?? '').split(':').map(Number)
      if (year && month && date && hours !== undefined && minutes !== undefined) onPick(new Date(year, month - 1, date, hours, minutes))
    },
    style: { padding: '6px 10px', borderRadius: 10, border: `1px solid ${colors.input}`, background: 'transparent', color: colors.foreground, colorScheme: scheme, fontSize: 16, fontFamily: fonts.regular },
  })
}
