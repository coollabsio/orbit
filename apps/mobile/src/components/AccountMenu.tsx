import type { Href } from 'expo-router'
import { type ReactNode, useState } from 'react'
import { Modal, Pressable, ScrollView, View } from 'react-native'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

import { hasCustomStatus, PRESENCE_OPTIONS, withoutCustomStatus, withPresence } from '../data/status'
import { useOwnStatus, useSetStatus } from '../hooks/account'
import { useOrbit } from '../orbit-context'
import { errorMessage } from '../session-context'
import { useTheme } from '../theme'
import { Icon, type IconName, type PickerSurface, StatusMark, Text } from '../ui'
import { Emoji } from './chat/EmojiPicker'
import { CustomStatusSheet } from './CustomStatusSheet'

const SURFACE: Record<PickerSurface, string> = { popover: 'bg-popover', sidebarPopover: 'bg-sidebar-popover' }

/**
 * The web's `UserMenu` as a bottom sheet: who you are, your presence and custom status, the account pages and Log out.
 * A row closes the sheet, then runs; Log out stays open until it is done or failed. `surface: 'sidebarPopover'` for
 * the drawer's sheet, like the web sidebar's menus.
 */
export function AccountMenu({
  visible,
  onClose,
  onNavigate,
  surface = 'popover',
}: {
  visible: boolean
  onClose: () => void
  /** Opens an account page (the drawer closes itself first). */
  onNavigate: (href: Href) => void
  surface?: PickerSurface
}) {
  const { colors } = useTheme()
  const insets = useSafeAreaInsets()
  const { user, signOut } = useOrbit()
  const status = useOwnStatus()
  const setStatus = useSetStatus()
  const [editing, setEditing] = useState(false)
  const [signingOut, setSigningOut] = useState(false)
  const [signOutError, setSignOutError] = useState<string | null>(null)
  const custom = hasCustomStatus(status)
  const sheetColor = surface === 'sidebarPopover' ? colors.sidebarPopover : colors.popover

  const run = (action: () => void) => () => {
    onClose()
    action()
  }
  const close = () => {
    if (signingOut) return
    setSignOutError(null)
    onClose()
  }

  return (
    <>
      <Modal visible={visible} transparent animationType="slide" onRequestClose={close} statusBarTranslucent>
        <Pressable accessibilityLabel="Close" onPress={close} className="flex-1 bg-foreground/40 dark:bg-black/40" />
        <View accessibilityViewIsModal className={`max-h-[85%] rounded-t-[12px] border-t border-border p-2 ${SURFACE[surface]}`} style={{ paddingBottom: Math.max(8, insets.bottom) }}>
          <ScrollView bounces={false}>
            <View className="min-w-0 px-3 pt-1 pb-2">
              <Text numberOfLines={1} className="font-noto-semibold text-[15px] leading-[20px]">
                {user.display_name}
              </Text>
              {custom ? (
                <View className="min-w-0 flex-row items-center gap-1">
                  {status.emoji ? <Emoji value={status.emoji} size={16} /> : null}
                  {status.text ? (
                    <Text numberOfLines={1} className="flex-1 text-[13px] leading-[18px] text-muted-foreground">
                      {status.text}
                    </Text>
                  ) : null}
                </View>
              ) : (
                <Text numberOfLines={1} className="text-[13px] leading-[18px] text-muted-foreground/70">
                  {user.email}
                </Text>
              )}
            </View>
            <Separator />
            <View accessibilityRole="radiogroup" accessibilityLabel="Status">
              {PRESENCE_OPTIONS.map((option) => (
                <Row
                  key={option.value}
                  label={option.label}
                  description={option.description}
                  leading={<StatusMark presence={option.value} size={10} surface={sheetColor} />}
                  selected={status.presence === option.value}
                  onPress={run(() => {
                    if (option.value !== status.presence) setStatus.mutate(withPresence(status, option.value))
                  })}
                />
              ))}
            </View>
            <Separator />
            <Row icon={custom ? 'edit' : 'smile'} label={custom ? 'Edit custom status' : 'Set custom status…'} onPress={run(() => setEditing(true))} />
            {custom ? <Row icon="x" label="Clear custom status" onPress={run(() => setStatus.mutate(withoutCustomStatus(status)))} /> : null}
            <Separator />
            <Row icon="user" label="Account settings" onPress={run(() => onNavigate('/settings/profile'))} />
            <Row icon="brush" label="Appearance" onPress={run(() => onNavigate('/settings/appearance'))} />
            <Separator />
            <Row
              icon="logout"
              label={signingOut ? 'Logging out…' : 'Log out'}
              destructive
              disabled={signingOut}
              onPress={() => {
                setSigningOut(true)
                setSignOutError(null)
                signOut().catch((error: unknown) => {
                  setSigningOut(false)
                  setSignOutError(errorMessage(error))
                })
              }}
            />
            {signOutError ? (
              <Text accessibilityRole="alert" className="px-3 py-1.5 text-[13px] leading-[18px] text-destructive">
                {`Could not log out. ${signOutError}`}
              </Text>
            ) : null}
          </ScrollView>
        </View>
      </Modal>
      {editing ? <CustomStatusSheet onClose={() => setEditing(false)} /> : null}
    </>
  )
}

function Separator() {
  return <View className="-mx-2 my-1 h-px bg-border" />
}

/** A menu row: an icon (or the presence dot), the label with an optional line under it, a check on the current choice. */
function Row({
  label,
  description,
  icon,
  leading,
  selected,
  destructive = false,
  disabled = false,
  onPress,
}: {
  label: string
  description?: string
  icon?: IconName
  leading?: ReactNode
  selected?: boolean
  destructive?: boolean
  disabled?: boolean
  onPress: () => void
}) {
  const { colors } = useTheme()
  return (
    <Pressable
      accessibilityRole={selected === undefined ? 'button' : 'radio'}
      accessibilityLabel={description ? `${label}. ${description}` : label}
      accessibilityState={{ disabled, ...(selected === undefined ? {} : { checked: selected }) }}
      disabled={disabled}
      onPress={onPress}
      className={`min-h-11 flex-row items-center gap-3 rounded-md px-3 py-1.5 active:bg-muted ${disabled ? 'opacity-50' : 'opacity-100'}`}
    >
      <View className="w-5 items-center">{leading ?? (icon ? <Icon name={icon} size={18} color={destructive ? colors.destructive : colors.mutedForeground} /> : null)}</View>
      <View className="min-w-0 flex-1">
        <Text className={`text-[14px] leading-[19px] ${destructive ? 'text-destructive' : 'text-popover-foreground'}`}>{label}</Text>
        {description ? <Text className="text-[12px] leading-[16px] text-muted-foreground">{description}</Text> : null}
      </View>
      {selected ? <Icon name="check" size={18} color={colors.primary} /> : null}
    </Pressable>
  )
}
