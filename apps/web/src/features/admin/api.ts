import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { changeUserEmail, createBackup, createRecoveryLink, deleteBackup, getInstanceSettings, getJobLimits, saveJobLimits, listAdminUsers, listBackups, listGlobalAudit, removeS3, removeSignInProvider, removeSmtp, resetTwoFactor, saveLimits, saveS3, saveSignInProvider, saveStorageOptions, setInstanceAdmin, startImageCompression, saveSmtp, sendTestEmail, setAccountSuspension, setRegistration } from '@orbit/api/generated/sdk.gen'
import type { InstanceSettingsView, JobLimitsBody, LimitsView, Provider, S3Body, SignInProviderBody, SmtpBody, StorageOptionsBody } from '@orbit/api/generated/types.gen'

/** Accounts on this instance in cursor pages; `search` matches part of the name or email. Admins only. */
export function useAdminUsers(search: string) {
  return useInfiniteQuery({
    queryKey: [...queryKeys.adminUsers, search],
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }) => {
      const { data } = await listAdminUsers({ client: apiClient, throwOnError: true, query: { q: search || undefined, cursor: pageParam } })
      return data
    },
    getNextPageParam: (last) => last.next_cursor ?? undefined,
    select: (data) => data.pages.flatMap((page) => page.items),
    placeholderData: keepPreviousData,
  })
}

export function useSetSuspension() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ userId, suspended }: { userId: string; suspended: boolean }) => {
      await setAccountSuspension({ client: apiClient, throwOnError: true, path: { user_id: userId }, body: { suspended } })
    },
    onSettled: () => Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.adminUsers }),
      queryClient.invalidateQueries({ queryKey: queryKeys.adminAudit }),
    ]),
  })
}

export function useSetInstanceAdmin() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ userId, admin }: { userId: string; admin: boolean }) => {
      await setInstanceAdmin({ client: apiClient, throwOnError: true, path: { user_id: userId }, body: { admin } })
    },
    onSettled: () => Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.adminUsers }),
      queryClient.invalidateQueries({ queryKey: queryKeys.adminAudit }),
    ]),
  })
}

/** Turns off the authenticator app of someone who lost it and their recovery codes. */
export function useResetTwoFactor() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (userId: string) => {
      await resetTwoFactor({ client: apiClient, throwOnError: true, path: { user_id: userId } })
    },
    onSettled: () => Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.adminUsers }),
      queryClient.invalidateQueries({ queryKey: queryKeys.adminAudit }),
    ]),
  })
}

export function useCreateRecoveryLink() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (userId: string) => {
      const { data } = await createRecoveryLink({ client: apiClient, throwOnError: true, path: { user_id: userId } })
      return data
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: queryKeys.adminAudit }),
  })
}

/** Changes an account's email at once, with no confirmation email; the account is signed out everywhere. */
export function useChangeUserEmail() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ userId, email }: { userId: string; email: string }) => {
      await changeUserEmail({ client: apiClient, throwOnError: true, path: { user_id: userId }, body: { email } })
    },
    onSettled: () => Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.adminUsers }),
      queryClient.invalidateQueries({ queryKey: queryKeys.adminAudit }),
      // An admin may change their own email here.
      queryClient.invalidateQueries({ queryKey: queryKeys.currentUser }),
    ]),
  })
}

/** Security events of every workspace, newest first, in cursor pages. */
export function useGlobalAudit() {
  return useInfiniteQuery({
    queryKey: queryKeys.adminAudit,
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }) => {
      const { data } = await listGlobalAudit({ client: apiClient, throwOnError: true, query: { cursor: pageParam } })
      return data
    },
    getNextPageParam: (last) => last.next_cursor ?? undefined,
    select: (data) => data.pages.flatMap((page) => page.items),
  })
}

export function useBackups() {
  return useQuery({
    queryKey: queryKeys.backups,
    queryFn: async () => {
      const { data } = await listBackups({ client: apiClient, throwOnError: true })
      return data.items
    },
  })
}

export function useCreateBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async () => {
      const { data } = await createBackup({ client: apiClient, throwOnError: true })
      if (!data) throw new Error('Backup response was empty.')
      return data
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: queryKeys.backups }),
  })
}

export function useDeleteBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (backupId: string) => {
      await deleteBackup({ client: apiClient, throwOnError: true, path: { backup_id: backupId } })
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: queryKeys.backups }),
  })
}

export function useInstanceSettings() {
  return useQuery({
    queryKey: queryKeys.adminSettings,
    queryFn: async () => {
      const { data } = await getInstanceSettings({ client: apiClient, throwOnError: true })
      return data
    },
    // Shows the progress while attachment files move between this server and S3, or images are compressed.
    refetchInterval: (query) => {
      const storage = query.state.data?.storage
      if (storage?.image_compression?.running) return 2000
      return storage?.files_to_move ? 5000 : false
    },
  })
}

/** Each settings write answers with the whole settings; the sign-in options follow from them. */
function useSettingsWrite<T>(write: (input: T) => Promise<InstanceSettingsView>) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: write,
    onSuccess: (settings) => {
      queryClient.setQueryData(queryKeys.adminSettings, settings)
      void queryClient.invalidateQueries({ queryKey: queryKeys.authOptions })
    },
  })
}

export function useSetRegistration() {
  return useSettingsWrite(async (open: boolean) => {
    const { data } = await setRegistration({ client: apiClient, throwOnError: true, body: { open } })
    return data
  })
}

export function useSaveSmtp() {
  return useSettingsWrite(async (body: SmtpBody) => {
    const { data } = await saveSmtp({ client: apiClient, throwOnError: true, body })
    return data
  })
}

export function useRemoveSmtp() {
  return useSettingsWrite(async () => {
    const { data } = await removeSmtp({ client: apiClient, throwOnError: true })
    return data
  })
}

export function useSendTestEmail() {
  return useMutation({
    mutationFn: async () => {
      await sendTestEmail({ client: apiClient, throwOnError: true })
    },
  })
}

export function useSaveS3() {
  return useSettingsWrite(async (body: S3Body) => {
    const { data } = await saveS3({ client: apiClient, throwOnError: true, body })
    return data
  })
}

export function useRemoveS3() {
  return useSettingsWrite(async () => {
    const { data } = await removeS3({ client: apiClient, throwOnError: true })
    return data
  })
}

/** Saves a Google or GitHub client; an empty `client_secret` keeps the saved one. */
export function useSaveSignInProvider() {
  return useSettingsWrite(async ({ provider, body }: { provider: Provider; body: SignInProviderBody }) => {
    const { data } = await saveSignInProvider({ client: apiClient, throwOnError: true, path: { provider }, body })
    return data
  })
}

export function useRemoveSignInProvider() {
  return useSettingsWrite(async (provider: Provider) => {
    const { data } = await removeSignInProvider({ client: apiClient, throwOnError: true, path: { provider } })
    return data
  })
}

export function useSaveStorageOptions() {
  return useSettingsWrite(async (body: StorageOptionsBody) => {
    const { data } = await saveStorageOptions({ client: apiClient, throwOnError: true, body })
    return data
  })
}

export function useSaveLimits() {
  return useSettingsWrite(async (body: LimitsView) => {
    const { data } = await saveLimits({ client: apiClient, throwOnError: true, body })
    return data
  })
}

export function useStartImageCompression() {
  return useSettingsWrite(async () => {
    const { data } = await startImageCompression({ client: apiClient, throwOnError: true })
    return data
  })
}

/** How many background jobs of each kind customers start may run at once. */
export function useJobLimits() {
  return useQuery({
    queryKey: queryKeys.adminJobLimits,
    queryFn: async () => {
      const { data } = await getJobLimits({ client: apiClient, throwOnError: true })
      return data
    },
  })
}

export function useSaveJobLimits() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: JobLimitsBody) => {
      const { data } = await saveJobLimits({ client: apiClient, throwOnError: true, body })
      return data
    },
    onSuccess: (limits) => queryClient.setQueryData(queryKeys.adminJobLimits, limits),
    onSettled: () => queryClient.invalidateQueries({ queryKey: queryKeys.adminAudit }),
  })
}
