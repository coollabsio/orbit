import { type ReactNode, useState } from 'react'
import { Pressable, View } from 'react-native'

import { commitTyped, isValidAddress } from '../../data/mail-compose'
import { useContacts } from '../../hooks/mail'
import { useTheme } from '../../theme'
import { Icon, Text } from '../../ui'
import { TextInput } from '../../ui/scaled'

/**
 * A To, Cc or Bcc line: the recipients as chips (an invalid one in red), the text being typed, and the people the user
 * mailed before as suggestions. A comma, a semicolon, Return or leaving the field turns the text into chips.
 */
export function RecipientField({ label, value, onChange, typed, onTyped, accessory }: {
  label: string
  value: string[]
  onChange: (next: string[]) => void
  /** The text not yet a chip: the screen commits it before saving or sending. */
  typed: string
  onTyped: (text: string) => void
  accessory?: ReactNode
}) {
  const { colors, scheme } = useTheme()
  const [focused, setFocused] = useState(false)
  const contacts = useContacts(focused ? typed : '')
  const commit = (text: string) => {
    if (text.trim()) onChange(commitTyped(value, text))
    onTyped('')
  }
  const suggestions = (contacts.data ?? []).filter((contact) => !value.some((address) => address.toLowerCase() === contact.address.toLowerCase())).slice(0, 5)
  return (
    <View className="border-b border-border">
      <View className="min-h-11 flex-row flex-wrap items-center gap-1.5 px-3 py-1.5">
        <Text className="text-[14px] text-muted-foreground">{label}</Text>
        {value.map((address) => {
          const valid = isValidAddress(address)
          return (
            <Pressable
              key={address}
              accessibilityRole="button"
              accessibilityLabel={`Remove ${address}${valid ? '' : ', not a valid address'}`}
              onPress={() => onChange(value.filter((entry) => entry !== address))}
              className={`h-7 flex-row items-center gap-1 rounded-full border px-2.5 ${valid ? 'border-border bg-muted' : 'border-destructive bg-destructive/10'}`}
            >
              <Text numberOfLines={1} className={`shrink text-[13px] ${valid ? '' : 'text-destructive'}`}>
                {address}
              </Text>
              <Icon name="close" size={12} color={valid ? colors.mutedForeground : colors.destructive} />
            </Pressable>
          )
        })}
        <TextInput
          value={typed}
          onChangeText={(text) => {
            if (/[,;\n]$/.test(text)) commit(text)
            else onTyped(text)
          }}
          onSubmitEditing={() => commit(typed)}
          onFocus={() => setFocused(true)}
          onBlur={() => {
            setFocused(false)
            commit(typed)
          }}
          submitBehavior="submit"
          keyboardType="email-address"
          autoCapitalize="none"
          autoCorrect={false}
          autoComplete="email"
          keyboardAppearance={scheme}
          selectionColor={colors.primary}
          accessibilityLabel={label}
          className="min-w-24 flex-1 min-h-8 font-noto text-foreground"
        />
        {accessory}
      </View>
      {focused && suggestions.length > 0 ? (
        <View className="border-t border-border bg-popover">
          {suggestions.map((contact) => (
            <Pressable key={contact.address} accessibilityRole="button" onPress={() => commit(contact.address)} className="min-h-11 justify-center px-3 py-1.5 active:bg-muted">
              <Text numberOfLines={1} className="text-[14px]">
                {contact.name || contact.address}
              </Text>
              {contact.name ? (
                <Text numberOfLines={1} className="text-[12px] text-muted-foreground">
                  {contact.address}
                </Text>
              ) : null}
            </Pressable>
          ))}
        </View>
      ) : null}
    </View>
  )
}
