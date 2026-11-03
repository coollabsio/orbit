import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { AdminShell, FieldLabel, Note, Segmented, SettingsCard } from '../../../../components/settings/Shell'
import { SMTP_DEFAULT_PORT, SMTP_SECURITY_OPTIONS, smtpBody, smtpDraft, type SmtpView } from '../../../../data/admin-api'
import { useAuthOptions } from '../../../../hooks/account'
import { useInstanceSettings, useRemoveSmtp, useSaveSmtp, useSendTestEmail } from '../../../../hooks/admin'
import { useOrbit } from '../../../../orbit-context'
import { errorMessage } from '../../../../session-context'
import { Button, Input, Text } from '../../../../ui'

/** The web's Admin → Email: the mail server for invitations, sign-up and password reset. */
export default function AdminEmailScreen() {
  const settings = useInstanceSettings()
  return (
    <AdminShell refreshing={settings.isRefetching} onRefresh={() => void settings.refetch()}>
      {settings.isPending ? <SettingsCard title="Email"><Note>Loading settings…</Note></SettingsCard> : null}
      {settings.isError ? (
        <SettingsCard title="Email">
          <View className="flex-row items-center gap-2">
            <Note error>Settings could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void settings.refetch()} />
          </View>
        </SettingsCard>
      ) : null}
      {settings.data ? <EmailCard smtp={settings.data.smtp ?? null} /> : null}
    </AdminShell>
  )
}

function EmailCard({ smtp }: { smtp: SmtpView | null }) {
  const { user } = useOrbit()
  // Orbit Cloud always needs a mail server: it can be replaced, not removed.
  const cloud = useAuthOptions().data?.billing_enabled ?? false
  const save = useSaveSmtp()
  const remove = useRemoveSmtp()
  const test = useSendTestEmail()
  const [draft, setDraft] = useState(() => smtpDraft(smtp))
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const edit = (change: Partial<typeof draft>) => {
    save.reset()
    setDraft((current) => ({ ...current, ...change }))
  }
  const set = (field: keyof typeof draft) => (value: string) => edit({ [field]: value })
  // after a save or a removal the form shows what the server stored
  const stored = { onSuccess: (settings: { smtp?: SmtpView | null }) => setDraft(smtpDraft(settings.smtp ?? null)) }
  const ready = draft.host.trim() !== '' && draft.port.trim() !== '' && draft.from_address.trim() !== ''

  return (
    <SettingsCard
      title="Email"
      description="The SMTP server for invitations, sign-up links, password reset links and email changes. Orbit sends no other email."
      actions={
        smtp ? (
          <>
            <Button label={test.isPending ? 'Sending…' : 'Send test email'} variant="outline" size="sm" disabled={test.isPending} onPress={() => test.mutate()} />
            {cloud ? null : (
              <Button
                label="Remove"
                variant="destructive"
                size="sm"
                disabled={remove.isPending}
                onPress={() =>
                  setConfirm({
                    title: 'Remove the mail server?',
                    description: 'Orbit stops sending invitations and password reset links by email, and registration closes.',
                    confirmLabel: 'Remove',
                    danger: true,
                    onConfirm: () => remove.mutate(undefined, stored),
                  })
                }
              />
            )}
          </>
        ) : undefined
      }
    >
      <View className="gap-4">
        <Input label="Host" required placeholder="smtp.example.com" value={draft.host} onChangeText={set('host')} autoCapitalize="none" autoCorrect={false} keyboardType="url" />
        <View className="gap-2">
          <FieldLabel>Encryption</FieldLabel>
          <Segmented
            label="Encryption"
            value={draft.security}
            options={SMTP_SECURITY_OPTIONS}
            onChange={(security) => edit({ security, port: String(SMTP_DEFAULT_PORT[security]) })}
          />
          <Text className="text-[12px] text-muted-foreground">
            {draft.security === 'none' ? 'Only for a mail relay on the same network.' : `Usually port ${SMTP_DEFAULT_PORT[draft.security]}.`}
          </Text>
        </View>
        <Input label="Port" required keyboardType="number-pad" value={draft.port} onChangeText={set('port')} />
        <Input label="Username" value={draft.username} onChangeText={set('username')} autoCapitalize="none" autoCorrect={false} autoComplete="off" />
        <Input
          label="Password"
          secureTextEntry
          autoComplete="new-password"
          placeholder={smtp?.password_set ? 'Saved. Leave empty to keep it.' : undefined}
          value={draft.password}
          onChangeText={set('password')}
        />
        <Input label="From address" required placeholder="orbit@example.com" keyboardType="email-address" autoCapitalize="none" autoCorrect={false} value={draft.from_address} onChangeText={set('from_address')} />
        <Input label="From name" placeholder="Orbit" value={draft.from_name} onChangeText={set('from_name')} />
        {cloud && smtp ? <Note>Orbit Cloud needs a mail server. Save a different one instead.</Note> : null}
        {save.isError ? <Note error>{errorMessage(save.error)}</Note> : null}
        {remove.isError ? <Note error>{errorMessage(remove.error)}</Note> : null}
        {test.isError ? <Note error>{errorMessage(test.error)}</Note> : null}
        {test.isSuccess ? <Note>Test email sent to {user.email}.</Note> : null}
        {save.isSuccess ? <Note>Mail settings saved.</Note> : null}
        <View className="flex-row justify-end">
          <Button label={save.isPending ? 'Saving…' : 'Save mail settings'} disabled={save.isPending || !ready} onPress={() => save.mutate(smtpBody(draft), stored)} />
        </View>
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}
