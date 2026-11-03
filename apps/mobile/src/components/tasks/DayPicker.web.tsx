import { createElement } from 'react'

import { fonts, useTheme } from '../../theme'

const isoDay = (day: Date) => `${day.getFullYear()}-${String(day.getMonth() + 1).padStart(2, '0')}-${String(day.getDate()).padStart(2, '0')}`

/** The browser's date field: the system picker has no web version (the web preview of the app). */
export function DayPicker({ value, onPick }: { value: Date; onPick: (day: Date) => void; onCancel: () => void }) {
  const { colors, scheme } = useTheme()
  return createElement('input', {
    type: 'date',
    'aria-label': 'Due date',
    defaultValue: isoDay(value),
    onChange: (event: { target: { value: string } }) => {
      const [year, month, day] = event.target.value.split('-').map(Number)
      if (year && month && day) onPick(new Date(year, month - 1, day))
    },
    style: { margin: '4px 16px 8px', padding: '6px 10px', borderRadius: 10, border: `1px solid ${colors.input}`, background: 'transparent', color: colors.foreground, colorScheme: scheme, fontSize: 16, fontFamily: fonts.regular },
  })
}
