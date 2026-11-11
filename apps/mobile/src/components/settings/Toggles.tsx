import { Pressable, View } from 'react-native'

import { Text } from '../../ui'

export interface ToggleOption<T extends string> {
  value: T
  label: string
  disabled?: boolean
}

/**
 * The web's joined outline `ToggleGroup` (`size="sm" spacing={0}`): 28px segments sharing their borders. The chosen one
 * is muted, or primary with `filled` (the access picker of the API token form, whose unavailable levels fade to 25%).
 */
export function Toggles<T extends string>({ value, options, onChange, label, filled = false }: { value: T; options: readonly ToggleOption<T>[]; onChange: (value: T) => void; label: string; filled?: boolean }) {
  return (
    <View accessibilityRole="radiogroup" accessibilityLabel={label} className="flex-row">
      {options.map((option, index) => {
        const selected = option.value === value
        const first = index === 0
        const last = index === options.length - 1
        const fill = selected ? (filled ? 'bg-primary border-primary' : 'bg-muted border-input') : 'border-input active:bg-muted'
        return (
          <Pressable
            key={option.value}
            accessibilityRole="radio"
            accessibilityLabel={option.label}
            accessibilityState={{ selected, disabled: option.disabled }}
            disabled={option.disabled}
            onPress={() => onChange(option.value)}
            className={`h-7 min-w-7 items-center justify-center border px-2 ${first ? 'rounded-l-lg' : 'border-l-0'} ${last ? 'rounded-r-lg' : ''} ${fill} ${option.disabled ? (filled ? 'opacity-25' : 'opacity-50') : ''}`}
          >
            <Text className={`font-noto-medium text-[12.8px] leading-[16px] ${selected && filled ? 'text-primary-foreground' : 'text-foreground'}`}>{option.label}</Text>
          </Pressable>
        )
      })}
    </View>
  )
}
