import { useEffect, useState } from 'react'
import { Share, View } from 'react-native'
import { TextInput } from '../../../../ui/scaled'

import { ActionSheet, ConfirmDialog, type ConfirmRequest, Dialog, type SheetAction } from '../../../../components/settings/Dialog'
import { AdminShell, Badge, type BadgeTone, Note, RowText, SettingsCard, SettingsRow } from '../../../../components/settings/Shell'
import { type AdminAction, adminActionBlocked, adminActions, adminBadges, type AdminUser } from '../../../../data/admin-api'
import { shortDate } from '../../../../data/format'
import { useAuthOptions } from '../../../../hooks/account'
import { useAdminUserWrites, useAdminUsers } from '../../../../hooks/admin'
import { useOrbit } from '../../../../orbit-context'
import { errorMessage } from '../../../../session-context'
import { alpha, useTheme } from '../../../../theme'
import { Avatar, Button, Icon, Input } from '../../../../ui'

const BADGE_TONE: Record<string, BadgeTone> = { Root: 'primary', Admin: 'secondary', '2FA': 'outline', Suspended: 'destructive', Deleted: 'secondary' }

/** The web's Admin → Users: every account on the instance, searched on the server, with the Manage menu. */
export default function AdminUsersScreen() {
  const { user: viewer, setUser } = useOrbit()
  const { colors } = useTheme()
  const [query, setQuery] = useState('')
  const [search, setSearch] = useState('')
  // the web waits 250 ms after the last keystroke before it searches
  useEffect(() => {
    const timer = setTimeout(() => setSearch(query.trim()), 250)
    return () => clearTimeout(timer)
  }, [query])
  const users = useAdminUsers(search)
  const writes = useAdminUserWrites()
  const options = useAuthOptions().data
  const cloud = options?.billing_enabled === true
  const [managing, setManaging] = useState<AdminUser | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [link, setLink] = useState<{ user: AdminUser; url: string; expiresAt: string } | null>(null)
  const [done, setDone] = useState<string | null>(null)
  const [emailUser, setEmailUser] = useState<AdminUser | null>(null)
  const [newEmail, setNewEmail] = useState('')
  const list = users.data?.pages.flatMap((page) => page.items) ?? []
  const failed = [writes.suspend, writes.setAdmin, writes.resetTwoFactor, writes.recoveryLink].find((write) => write.isError)?.error

  const run = (user: AdminUser, action: AdminAction) => {
    switch (action) {
      case 'change-email':
        writes.changeEmail.reset()
        setNewEmail('')
        return setEmailUser(user)
      case 'make-admin':
        return setConfirm({
          title: `Make ${user.display_name} an instance admin?`,
          description: 'They can manage accounts (except you and other admins), email, registration, backups and the audit log.',
          confirmLabel: 'Make admin',
          onConfirm: () => writes.setAdmin.mutate({ userId: user.id, isAdmin: true }, { onSuccess: () => setDone(`${user.display_name} is now an admin`) }),
        })
      case 'remove-admin':
        return writes.setAdmin.mutate({ userId: user.id, isAdmin: false }, { onSuccess: () => setDone(`${user.display_name} is no longer an admin`) })
      case 'recovery-link':
        return writes.recoveryLink.mutate(user.id, {
          onSuccess: (created) => {
            if (created.url) setLink({ user, url: created.url, expiresAt: created.expires_at })
            else setDone(`Password reset email sent to ${created.sent_to ?? user.email}`)
          },
        })
      case 'reset-two-factor':
        return setConfirm({
          title: `Reset two-factor sign-in for ${user.display_name}?`,
          description: `Their authenticator app and recovery codes stop working, and they sign in with only their password until they set it up again.${cloud ? ' They get an email about it.' : ''} Do this only after you have confirmed who is asking.`,
          confirmLabel: 'Reset two-factor',
          danger: true,
          onConfirm: () => writes.resetTwoFactor.mutate(user.id, { onSuccess: () => setDone(`Two-factor sign-in is off for ${user.display_name}`) }),
        })
      case 'suspend':
        return setConfirm({
          title: `Suspend ${user.display_name}?`,
          description: 'They are signed out everywhere and cannot sign in until you reinstate the account. Their API tokens stop working.',
          confirmLabel: 'Suspend account',
          danger: true,
          onConfirm: () => writes.suspend.mutate({ userId: user.id, suspended: true }, { onSuccess: () => setDone(`${user.display_name} is suspended`) }),
        })
      case 'reinstate':
        return writes.suspend.mutate({ userId: user.id, suspended: false }, { onSuccess: () => setDone(`${user.display_name} is reinstated`) })
    }
  }
  const saveEmail = (user: AdminUser) => {
    const email = newEmail.trim()
    writes.changeEmail.mutate({ userId: user.id, email }, {
      onSuccess: () => {
        if (user.id === viewer.id) setUser({ ...viewer, email })
        setEmailUser(null)
        setDone(`The email of ${user.display_name} is now ${email}`)
      },
    })
  }
  const LABELS: Record<AdminAction, string> = {
    'change-email': 'Change email',
    'make-admin': 'Make admin',
    'remove-admin': 'Remove admin',
    'recovery-link': cloud ? 'Send password reset email' : 'Create password recovery link',
    'reset-two-factor': 'Reset two-factor',
    suspend: 'Suspend account',
    reinstate: 'Reinstate account',
  }
  const sheet: SheetAction[] = managing
    ? adminActions(managing, viewer).map((action) => ({
        label: LABELS[action],
        destructive: action === 'suspend' || action === 'reset-two-factor',
        disabled: adminActionBlocked(action, options) !== null,
        detail: adminActionBlocked(action, options) ?? undefined,
        onPress: () => {
          setDone(null)
          run(managing, action)
        },
      }))
    : []

  return (
    <AdminShell refreshing={users.isRefetching} onRefresh={() => void users.refetch()}>
      <SettingsCard title="Users" description="Every account on this Orbit instance." flush>
        <View className="p-3 border-b border-border">
          <View className="h-8 flex-row items-center gap-2 px-2.5 rounded-lg border border-input">
            <Icon name="search" size={14} color={alpha(colors.mutedForeground, 0.7)} />
            <TextInput
              accessibilityLabel="Search users"
              placeholder="Search by name or email…"
              placeholderTextColor={colors.mutedForeground}
              value={query}
              onChangeText={setQuery}
              autoCapitalize="none"
              autoCorrect={false}
              className="font-noto text-[14px] leading-[1.45] flex-1 py-0 text-foreground"
            />
          </View>
        </View>
        {users.isPending ? <View className="p-4"><Note>Loading users…</Note></View> : null}
        {users.isError ? (
          <View className="p-4 flex-row items-center gap-2">
            <Note error>Users could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void users.refetch()} />
          </View>
        ) : null}
        {search && users.data && list.length === 0 ? <View className="p-4"><Note>No user matches "{search}".</Note></View> : null}
        {list.map((user, index) => (
          <SettingsRow key={user.id} first={index === 0}>
            <Avatar id={user.id} name={user.display_name || user.email} size={32} />
            <RowText
              title={user.display_name}
              // a deleted account's email is a placeholder that frees the address
              subtitle={`${user.deleted ? 'Anonymized. It can never sign in again.' : user.email}\n${user.workspace_count} ${user.workspace_count === 1 ? 'workspace' : 'workspaces'} · ${user.last_active_at ? `Active ${shortDate(user.last_active_at)}` : 'Never signed in'}`}
              badges={adminBadges(user).map((badge) => <Badge key={badge} label={badge} tone={BADGE_TONE[badge]} />)}
            />
            {adminActions(user, viewer).length > 0 ? <Button label="Manage" size="sm" variant="outline" onPress={() => setManaging(user)} /> : null}
          </SettingsRow>
        ))}
        {users.hasNextPage ? (
          <View className="p-2 items-center border-t border-border">
            <Button label={users.isFetchingNextPage ? 'Loading…' : 'Load more users'} variant="ghost" disabled={users.isFetchingNextPage} onPress={() => void users.fetchNextPage()} />
          </View>
        ) : null}
        {failed ? <View className="p-4 border-t border-border"><Note error>{errorMessage(failed)}</Note></View> : null}
        {done && !failed ? <View className="p-4 border-t border-border"><Note>{done}</Note></View> : null}
      </SettingsCard>

      <ActionSheet visible={managing !== null} title={managing?.display_name} actions={sheet} onClose={() => setManaging(null)} />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
      <Dialog
        visible={link !== null}
        title={`Recovery link for ${link?.user.display_name ?? ''}`}
        description={link ? `Send this link to ${link.user.email}. It sets a new password, works once, and expires at ${new Date(link.expiresAt).toLocaleTimeString()}.` : undefined}
        onClose={() => setLink(null)}
        footer={
          <>
            <Button label="Share link" icon="send" variant="outline" onPress={() => link && void Share.share({ message: link.url })} />
            <Button label="Done" onPress={() => setLink(null)} />
          </>
        }
      >
        <Input label="Recovery link" value={link?.url ?? ''} editable={false} selectTextOnFocus />
      </Dialog>
      <Dialog
        visible={emailUser !== null}
        title={`Change email of ${emailUser?.display_name ?? ''}`}
        description={cloud ? 'The person is signed out everywhere. The old and the new address get an email about the change.' : 'The person is signed out everywhere. No confirmation email is sent.'}
        onClose={() => setEmailUser(null)}
        footer={
          <>
            <Button label="Cancel" variant="ghost" disabled={writes.changeEmail.isPending} onPress={() => setEmailUser(null)} />
            <Button
              label={writes.changeEmail.isPending ? 'Saving…' : 'Change email'}
              disabled={writes.changeEmail.isPending || !newEmail.trim()}
              onPress={() => emailUser && saveEmail(emailUser)}
            />
          </>
        }
      >
        <Input
          label="New email"
          value={newEmail}
          onChangeText={setNewEmail}
          keyboardType="email-address"
          autoCapitalize="none"
          autoCorrect={false}
          autoFocus
          description={`Now ${emailUser?.email ?? ''}.`}
          error={writes.changeEmail.isError ? errorMessage(writes.changeEmail.error) : undefined}
        />
      </Dialog>
    </AdminShell>
  )
}
