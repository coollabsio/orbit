import { type ReactNode, useEffect, useState } from 'react'
import { KeyboardAvoidingView, Modal, Platform, Pressable, ScrollView, View } from 'react-native'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

import { useTheme } from '../../theme'
import { Button, Icon, type IconName, Input, Text } from '../../ui'

export interface SheetAction {
  label: string
  icon?: IconName
  destructive?: boolean
  disabled?: boolean
  /** A second line under the label, for example why the row is disabled. */
  detail?: string
  onPress: () => void
}

/** The web's dropdown menus at phone size: a bottom sheet of rows. A row closes the sheet, then runs its action. */
export function ActionSheet({ visible, title, actions, onClose }: { visible: boolean; title?: string; actions: readonly SheetAction[]; onClose: () => void }) {
  const { colors } = useTheme()
  const insets = useSafeAreaInsets()
  return (
    <Modal visible={visible} transparent animationType="fade" onRequestClose={onClose} statusBarTranslucent>
      <Pressable accessibilityLabel="Close" onPress={onClose} className="flex-1 bg-black/50" />
      <View className="absolute left-2 right-2 rounded-xl bg-popover border border-border py-1.5" style={{ bottom: 8 + insets.bottom }}>
        {title ? (
          <Text numberOfLines={1} className="font-noto-medium text-[12px] px-3.5 py-1.5 text-muted-foreground">
            {title}
          </Text>
        ) : null}
        {actions.map((action) => (
          <Pressable
            key={action.label}
            accessibilityRole="button"
            accessibilityLabel={action.label}
            accessibilityState={{ disabled: action.disabled }}
            disabled={action.disabled}
            onPress={() => {
              onClose()
              action.onPress()
            }}
            className={`min-h-11 flex-row items-center gap-2.5 mx-1.5 px-2 rounded-md active:bg-muted ${action.disabled ? 'opacity-50' : 'opacity-100'}`}
          >
            {action.icon ? <Icon name={action.icon} size={16} color={action.destructive ? colors.destructive : colors.mutedForeground} /> : null}
            <View className="py-1.5">
              <Text className={`font-noto-medium text-[14px] ${action.destructive ? 'text-destructive' : 'text-foreground'}`}>{action.label}</Text>
              {action.detail ? <Text className="font-noto text-[12px] text-muted-foreground">{action.detail}</Text> : null}
            </View>
          </Pressable>
        ))}
      </View>
    </Modal>
  )
}

/** The web `Modal`: centered on both axes, title, description, body and the footer buttons. */
export function Dialog({ visible, title, description, children, footer, onClose }: { visible: boolean; title: string; description?: string; children?: ReactNode; footer: ReactNode; onClose: () => void }) {
  return (
    <Modal visible={visible} transparent animationType="fade" onRequestClose={onClose} statusBarTranslucent>
      <KeyboardAvoidingView behavior={Platform.OS === 'ios' ? 'padding' : undefined} className="flex-1">
        <Pressable accessibilityLabel="Close" onPress={onClose} className="absolute inset-0 bg-black/50" />
        <View pointerEvents="box-none" className="flex-1 justify-center p-4">
          <View accessibilityRole="none" accessibilityLabel={title} className="max-h-[90%] rounded-xl bg-popover border border-border">
            <ScrollView contentContainerClassName="p-4 gap-3.5" keyboardShouldPersistTaps="handled">
              <View className="gap-1">
                <Text className="font-noto-semibold text-[16px]">{title}</Text>
                {description ? <Text className="text-[13px] leading-[19px] text-muted-foreground">{description}</Text> : null}
              </View>
              {children}
              <View className="flex-row flex-wrap justify-end gap-2">{footer}</View>
            </ScrollView>
          </View>
        </View>
      </KeyboardAvoidingView>
    </Modal>
  )
}

export interface ConfirmRequest {
  title: string
  description?: string
  confirmLabel: string
  danger?: boolean
  onConfirm: () => void
}

/** A confirmation before an action with an impact (`confirmAction` on the web). `request: null` hides it. */
export function ConfirmDialog({ request, onClose }: { request: ConfirmRequest | null; onClose: () => void }) {
  return (
    <Dialog
      visible={request !== null}
      title={request?.title ?? ''}
      description={request?.description}
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button
            label={request?.confirmLabel ?? 'Confirm'}
            variant={request?.danger ? 'destructive' : 'primary'}
            onPress={() => {
              onClose()
              request?.onConfirm()
            }}
          />
        </>
      }
    />
  )
}

export interface PromptRequest {
  title: string
  description?: string
  label: string
  initial?: string
  placeholder?: string
  confirmLabel: string
  secure?: boolean
  keyboard?: 'default' | 'email-address' | 'number-pad' | 'url'
  /** Resolves to close; rejects with the message to show under the field. */
  onSubmit: (value: string) => Promise<unknown>
}

/** A one-field form in a dialog (rename, new teamspace, confirm with your password …). `request: null` hides it. */
export function PromptDialog({ request, onClose }: { request: PromptRequest | null; onClose: () => void }) {
  const [value, setValue] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    setValue(request?.initial ?? '')
    setError(null)
    setBusy(false)
  }, [request])

  const submit = () => {
    if (!request || busy || !value.trim()) return
    setBusy(true)
    setError(null)
    request.onSubmit(request.secure ? value : value.trim()).then(onClose, (reason: unknown) => {
      setBusy(false)
      setError(reason instanceof Error ? reason.message : String(reason))
    })
  }

  return (
    <Dialog
      visible={request !== null}
      title={request?.title ?? ''}
      description={request?.description}
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={request?.confirmLabel ?? 'Save'} loading={busy} disabled={!value.trim()} onPress={submit} />
        </>
      }
    >
      <Input
        label={request?.label}
        value={value}
        onChangeText={setValue}
        placeholder={request?.placeholder}
        secureTextEntry={request?.secure}
        autoComplete={request?.secure ? 'current-password' : undefined}
        autoCapitalize={request?.secure || request?.keyboard === 'email-address' || request?.keyboard === 'url' ? 'none' : 'sentences'}
        keyboardType={request?.keyboard ?? 'default'}
        autoFocus
        returnKeyType="done"
        onSubmitEditing={submit}
        error={error ?? undefined}
      />
    </Dialog>
  )
}
