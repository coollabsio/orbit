/** The account and workspace settings queries and writes, over `data/account-api.ts`. */
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import * as ImagePicker from 'expo-image-picker'
import { useEffect, useState } from 'react'
import { Alert } from 'react-native'

import * as account from '../data/account-api'
import { keys } from '../data/keys'
import type { MemberRecord, User } from '../data/orbit'
import * as providers from '../data/sign-in-provider'
import { currentStatus, type OwnStatus, type UserStatus } from '../data/status'
import { useOrbit } from '../orbit-context'
import { uploadPart } from '../upload-part'

/**
 * Opens the photo library and answers with the chosen image as an upload part, or `null` when the person cancels.
 * `square` crops it like the web's profile picture; a file over `maxBytes` throws the message to show.
 */
export async function pickImage({ square, maxBytes }: { square: boolean; maxBytes?: number }): Promise<account.UploadFile | null> {
  const result = await ImagePicker.launchImageLibraryAsync({ mediaTypes: ['images'], allowsEditing: square, aspect: square ? [1, 1] : undefined, quality: square ? 0.3 : 0.7 })
  const asset = result.canceled ? undefined : result.assets[0]
  if (!asset) return null
  if (maxBytes && asset.fileSize && asset.fileSize > maxBytes) {
    throw new Error(`This picture is ${account.formatBytes(asset.fileSize)}; the limit is ${account.formatBytes(maxBytes)}. Crop it closer or pick a smaller one.`)
  }
  return uploadPart(asset.uri, account.pickedImageName(asset))
}

/** The signed-in user changed (picture): the session and every members list show it. */
function useUserChanged() {
  const { setUser, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return (user: User) => {
    setUser(user)
    void queryClient.invalidateQueries({ queryKey: keys.members(workspaceId) })
  }
}

/** Uploads (an image) or removes (`null`) the profile picture. */
export function useSetProfilePicture() {
  const { client } = useOrbit()
  const changed = useUserChanged()
  return useMutation({
    mutationFn: (file: account.UploadFile | null) => (file ? account.uploadProfilePicture(client, file) : account.removeProfilePicture(client)),
    onSuccess: changed,
  })
}

/**
 * The signed-in user's own status, from the session user (where an invisible user is not offline). The custom status
 * goes away when its time comes: the hook renders again at that moment.
 */
export function useOwnStatus(): OwnStatus {
  const status = useOrbit().user.status
  const expiresAt = status.expires_at ?? null
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (expiresAt === null) return
    const delay = Math.max(0, Date.parse(expiresAt) - Date.now())
    if (!(delay <= 2 ** 31 - 1)) return
    const timer = setTimeout(() => setNow(Date.now()), delay + 50)
    return () => clearTimeout(timer)
  }, [expiresAt])
  return currentStatus(status, now)
}

/**
 * Sets the presence and the custom status. The session user shows it at once; a failure puts the old one back.
 * `alertOnError: false` leaves the error to the caller (the custom status dialog shows it and stays open).
 */
export function useSetStatus({ alertOnError = true }: { alertOnError?: boolean } = {}) {
  const { client, user, setUser } = useOrbit()
  return useMutation({
    mutationFn: (status: UserStatus) => account.setStatus(client, status),
    onMutate: (status) => {
      setUser({ ...user, status })
      return { previous: user }
    },
    onError: (_error, _status, context) => {
      if (context) setUser(context.previous)
      if (alertOnError) Alert.alert('Could not update your status', 'Try again.')
    },
    onSuccess: setUser,
  })
}

export function useChangePassword() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ current, next }: { current: string; next: string }) => account.changePassword(client, current, next),
    // the other devices are signed out
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: account.accountKeys.sessions }),
  })
}

export function useNotificationPrefs() {
  const { client } = useOrbit()
  return useQuery({ queryKey: account.accountKeys.notificationPrefs, queryFn: () => account.notificationPrefs(client) })
}

/** Saves the whole set. The switch moves at once and goes back if the server refuses, as on the web. */
export function useSaveNotificationPrefs() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  const key = account.accountKeys.notificationPrefs
  return useMutation({
    mutationFn: (prefs: account.NotificationPrefs) => account.saveNotificationPrefs(client, prefs),
    onMutate: async (prefs) => {
      await queryClient.cancelQueries({ queryKey: key })
      const previous = queryClient.getQueryData<account.NotificationPrefs>(key)
      queryClient.setQueryData(key, prefs)
      return { previous }
    },
    onError: (_error, _prefs, context) => {
      if (context?.previous) queryClient.setQueryData(key, context.previous)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: key }),
  })
}

export function useSessions() {
  const { client } = useOrbit()
  return useQuery({ queryKey: account.accountKeys.sessions, queryFn: async () => account.sortedSessions(await account.sessions(client)) })
}

/** Revokes sessions one by one; the error names how many failed. */
export function useRevokeSessions() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (ids: string[]) => {
      const results = await Promise.allSettled(ids.map((id) => account.revokeSession(client, id)))
      const failed = results.filter((result) => result.status === 'rejected').length
      if (failed > 0) throw new Error(`${failed} of ${ids.length} sessions could not be revoked.`)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: account.accountKeys.sessions }),
  })
}

export function useOAuthConnections() {
  const { client } = useOrbit()
  return useQuery({ queryKey: account.accountKeys.oauthConnections, queryFn: () => account.oauthConnections(client) })
}

export function useDisconnectOAuth() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => account.disconnectOAuth(client, id),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: account.accountKeys.oauthConnections }),
  })
}

export function useTwoFactor() {
  const { client } = useOrbit()
  return useQuery({ queryKey: account.accountKeys.twoFactor, queryFn: () => account.twoFactor(client) })
}

/** The two-factor writes, confirmed with the password (or a fresh provider sign-in without one): each refreshes the status. */
export function useTwoFactorWrites() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  const onSuccess = () => void queryClient.invalidateQueries({ queryKey: account.accountKeys.twoFactor })
  return {
    setup: useMutation({ mutationFn: (password?: string) => account.startTotpSetup(client, password) }),
    enable: useMutation({ mutationFn: (code: string) => account.turnOnTotp(client, code), onSuccess }),
    disable: useMutation({ mutationFn: (password?: string) => account.turnOffTwoFactor(client, password), onSuccess }),
    regenerate: useMutation({ mutationFn: (password?: string) => account.newRecoveryCodes(client, password), onSuccess }),
  }
}

export function useConnectedAccounts() {
  const { client } = useOrbit()
  return useQuery({ queryKey: account.accountKeys.connectedAccounts, queryFn: () => providers.connectedAccounts(client) })
}

export function useDisconnectAccount() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (provider: providers.Provider) => providers.disconnect(client, provider),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: account.accountKeys.connectedAccounts }),
  })
}

export function useWorkspaceStorage(offset = 0) {
  const { client, workspaceId } = useOrbit()
  return useQuery({
    queryKey: [...account.accountKeys.storage(workspaceId), offset],
    queryFn: () => account.workspaceStorage(client, workspaceId, offset),
    placeholderData: keepPreviousData,
  })
}

export function useDeleteStorageFile() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (fileId: string) => account.deleteStorageFile(client, workspaceId, fileId),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: account.accountKeys.storage(workspaceId) }),
  })
}

export function useWorkspaceBucket() {
  const { client, workspaceId, workspace } = useOrbit()
  return useQuery({
    enabled: workspace.permissions.includes('storage.manage'),
    queryKey: account.accountKeys.bucket(workspaceId),
    queryFn: () => account.workspaceBucket(client, workspaceId),
    refetchInterval: (query) => account.bucketPollInterval(query.state.data),
  })
}

/** Save and remove answer with the new bucket state, which the page shows at once. */
export function useWorkspaceBucketWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSuccess = (view: account.WorkspaceBucketView) => queryClient.setQueryData(account.accountKeys.bucket(workspaceId), view)
  return {
    save: useMutation({ mutationFn: (body: Parameters<typeof account.saveBucket>[2]) => account.saveBucket(client, workspaceId, body), onSuccess }),
    remove: useMutation({ mutationFn: () => account.removeBucket(client, workspaceId), onSuccess }),
  }
}

export function useRenameWorkspace() {
  const { client, workspace } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => account.renameWorkspace(client, workspace, name),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.workspaces }),
  })
}

export function useAuthOptions() {
  const { client } = useOrbit()
  return useQuery({ queryKey: account.accountKeys.authOptions, queryFn: () => account.authOptions(client), staleTime: Infinity })
}

export function useInvitations(enabled: boolean) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: account.accountKeys.invitations(workspaceId), queryFn: () => account.pendingInvitations(client, workspaceId), enabled })
}

/** Member and invitation writes. Each refreshes the members (invitations sit under the same prefix). */
export function useMemberWrites() {
  const { client, workspace, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSettled = () => void queryClient.invalidateQueries({ queryKey: keys.members(workspaceId) })
  const workspaceChanged = () => {
    onSettled()
    void queryClient.invalidateQueries({ queryKey: keys.workspaces })
  }
  return {
    changeRole: useMutation({ mutationFn: ({ member, role }: { member: MemberRecord; role: 'admin' | 'member' }) => account.changeRole(client, workspaceId, member, role), onSettled }),
    remove: useMutation({ mutationFn: (member: MemberRecord) => account.removeMembership(client, workspaceId, member), onSettled }),
    transfer: useMutation({ mutationFn: (member: MemberRecord) => account.transferOwnership(client, workspace, member), onSettled: workspaceChanged }),
    invite: useMutation({
      mutationFn: ({ email, role, delivery }: { email: string; role: 'admin' | 'member'; delivery: 'manual' | 'smtp' }) => account.invite(client, workspaceId, email, role, delivery),
      onSettled,
    }),
    revokeInvitation: useMutation({ mutationFn: (id: string) => account.revokeInvitation(client, workspaceId, id), onSettled }),
    /** Leaves the workspace: the workspace list drops it and the app opens the next one. */
    leave: useMutation({ mutationFn: (member: MemberRecord) => account.removeMembership(client, workspaceId, member), onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.workspaces }) }),
  }
}
