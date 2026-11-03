import { canDeleteStorageFile, storageFileLabel, storageFilesPage } from '@orbit/api/storage'
import type { StorageFile } from '@orbit/api/generated/types.gen'
import { router } from 'expo-router'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { ACCOUNT_NAV, Note, RowIcon, RowText, SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { formatBytes, storageUse } from '../../../data/account-api'
import { useAuthOptions, useDeleteStorageFile, useMemberWrites, useRenameWorkspace, useWorkspaceStorage } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { useMembers } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { useTheme } from '../../../theme'
import { Button, Icon, Input, Text } from '../../../ui'

/**
 * The dock's Settings: the web's workspace General page (name, storage, leave), and the account pages,
 * the server and Sign out, which the web keeps in the account menu.
 */
export default function GeneralScreen() {
  const { workspace, session, user, signOut } = useOrbit()
  const { colors } = useTheme()
  const [offset, setOffset] = useState(0)
  const storage = useWorkspaceStorage(offset)
  const files = storage.data?.largest_files ?? []
  const page = storageFilesPage(offset, files.length, storage.data?.file_count ?? 0)
  // a delete emptied the last page: go back one
  if (storage.data && !storage.isPlaceholderData && files.length === 0 && page.previous !== null) setOffset(page.previous)
  const [signingOut, setSigningOut] = useState(false)
  const [signOutError, setSignOutError] = useState<string | null>(null)

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} refreshing={storage.isRefetching} onRefresh={() => void storage.refetch()}>
      {workspace.permissions.includes('workspace.update') ? <WorkspaceName key={workspace.id} /> : null}

      <SettingsCard
        title="Storage"
        description="Files, emoji and stickers of this workspace. A file attached more than once counts once; a deleted file stops counting about a day after the trash is emptied."
        actions={workspace.permissions.includes('storage.manage') ? <Button label="Use your own bucket" variant="outline" size="sm" onPress={() => router.push('/settings/file-storage')} /> : undefined}
      >
        {storage.data ? <StorageBar {...storageUse(storage.data)} /> : <Note error={storage.isError}>{storage.isError ? 'Storage use could not be loaded.' : 'Loading storage…'}</Note>}
        {files.length ? (
          <LargestFiles files={files} page={page} loading={storage.isFetching} onPage={setOffset} />
        ) : null}
      </SettingsCard>

      <SettingsCard title="Account" description={`${user.email} on ${session.server}`} flush>
        {ACCOUNT_NAV.map((item, index) => (
          <SettingsRow key={item.href} first={index === 0} label={item.label} onPress={() => router.push(item.href)}>
            <RowIcon icon={item.icon} />
            <RowText title={item.label} />
            <Icon name="chevronRight" size={14} color={colors.mutedForeground} />
          </SettingsRow>
        ))}
        <View className="p-4 border-t border-border gap-2 items-start">
          <Button
            label="Sign out"
            variant="outline"
            icon="logout"
            loading={signingOut}
            onPress={() => {
              setSigningOut(true)
              setSignOutError(null)
              signOut().catch((error: unknown) => {
                setSigningOut(false)
                setSignOutError(errorMessage(error))
              })
            }}
          />
          {signOutError ? <Note error>{signOutError}</Note> : null}
        </View>
      </SettingsCard>

      <LeaveWorkspace key={`leave-${workspace.id}`} />
    </SettingsShell>
  )
}

function WorkspaceName() {
  const { workspace } = useOrbit()
  const rename = useRenameWorkspace()
  const [name, setName] = useState(workspace.name)
  const dirty = name.trim() !== workspace.name
  return (
    <SettingsCard title="Workspace" description="Rename this workspace.">
      <Input label="Name" required value={name} onChangeText={setName} maxLength={100} returnKeyType="done" onSubmitEditing={() => dirty && name.trim() && rename.mutate(name.trim())} />
      {rename.isError ? <View className="mt-3"><Note error>Workspace rename failed. {errorMessage(rename.error)}</Note></View> : null}
      {dirty ? (
        <View className="mt-4 flex-row gap-2 justify-end">
          <Button label="Discard" variant="outline" onPress={() => setName(workspace.name)} />
          <Button label="Save" loading={rename.isPending} disabled={!name.trim()} onPress={() => rename.mutate(name.trim())} />
        </View>
      ) : null}
    </SettingsCard>
  )
}

function StorageBar({ label, fraction }: { label: string; fraction: number | null }) {
  return (
    <View className="gap-2">
      <View className="flex-row justify-between gap-2">
        <Text className="text-[13px]">Used</Text>
        <Text className="text-[13px] text-muted-foreground tabular-nums">{label}</Text>
      </View>
      {fraction !== null ? (
        <View accessibilityRole="progressbar" accessibilityValue={{ min: 0, max: 100, now: Math.round(fraction * 100) }} className="h-1.5 rounded-full bg-muted overflow-hidden">
          <View className={`h-1.5 rounded-full ${fraction >= 0.9 ? 'bg-destructive' : 'bg-primary'}`} style={{ width: `${fraction * 100}%` }} />
        </View>
      ) : null}
    </View>
  )
}

/** Only admins get these from the server; they can delete a file (not one of an emoji or sticker). */
function LargestFiles({ files, page, loading, onPage }: { files: StorageFile[]; page: ReturnType<typeof storageFilesPage>; loading: boolean; onPage: (offset: number) => void }) {
  const deleteFile = useDeleteStorageFile()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  return (
    <View className="mt-5 gap-2">
      <Text className="text-[13px] font-medium">Largest files</Text>
      {deleteFile.isError ? <Note error>The file could not be deleted. {errorMessage(deleteFile.error)}</Note> : null}
      <View className="rounded-lg border border-border">
        {files.map((file, index) => {
          const { name, where } = storageFileLabel(file)
          return (
            <View key={file.id} className={`flex-row items-center gap-3 px-3 py-2 ${index > 0 ? 'border-t border-border' : ''}`}>
              <View className="flex-1 gap-0.5">
                <Text className="text-[13px]" numberOfLines={1}>{name}</Text>
                <Text className="text-xs text-muted-foreground" numberOfLines={1}>{where}</Text>
              </View>
              <Text className="text-[13px] text-muted-foreground tabular-nums">{formatBytes(file.byte_size)}</Text>
              {canDeleteStorageFile(file) ? (
                <Button
                  label="Delete"
                  variant="ghost"
                  size="sm"
                  disabled={deleteFile.isPending}
                  onPress={() =>
                    setConfirm({
                      title: `Delete ${name}?`,
                      description: 'It frees its storage at once. Tasks, pages and messages that use it show “Deleted file” instead. You cannot undo this.',
                      confirmLabel: 'Delete',
                      danger: true,
                      onConfirm: () => deleteFile.mutate(file.id),
                    })
                  }
                />
              ) : null}
            </View>
          )
        })}
      </View>
      {page.previous !== null || page.next !== null ? (
        <View className="flex-row items-center gap-2">
          <Text className="flex-1 text-xs text-muted-foreground tabular-nums">{page.label}</Text>
          <Button label="Previous" variant="outline" size="sm" disabled={page.previous === null || loading} onPress={() => page.previous !== null && onPage(page.previous)} />
          <Button label="Next" variant="outline" size="sm" disabled={page.next === null || loading} onPress={() => page.next !== null && onPage(page.next)} />
        </View>
      ) : null}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </View>
  )
}

/** Any member may leave; the owner transfers ownership first (`can_remove` on your own row). */
function LeaveWorkspace() {
  const { workspace, user } = useOrbit()
  const members = useMembers()
  const { leave } = useMemberWrites()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const membership = members.data?.find((member) => member.user_id === user.id)
  // Orbit Cloud refuses ownership transfer.
  const billing = useAuthOptions().data?.billing_enabled ?? false

  if (membership && !membership.can_remove) {
    return (
      <SettingsCard title="Leave workspace" description="You own this workspace.">
        <Note>
          {billing
            ? 'The owner cannot leave. Ownership transfer is not available on Orbit Cloud yet.'
            : 'To leave, first transfer ownership to another member in Members.'}
        </Note>
      </SettingsCard>
    )
  }
  return (
    <SettingsCard title="Leave workspace" description="Remove yourself from this workspace.">
      <View className="items-start gap-2">
        <Button
          label={leave.isPending ? 'Leaving…' : 'Leave workspace'}
          variant="destructive"
          disabled={!membership || leave.isPending}
          onPress={() =>
            setConfirm({
              title: `Leave ${workspace.name}?`,
              description: 'You lose access to its projects, tasks, docs and chat. To come back, someone must invite you again.',
              confirmLabel: 'Leave workspace',
              danger: true,
              onConfirm: () => membership && leave.mutate(membership),
            })
          }
        />
        {leave.isError ? <Note error>You could not leave the workspace. {errorMessage(leave.error)}</Note> : null}
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}
