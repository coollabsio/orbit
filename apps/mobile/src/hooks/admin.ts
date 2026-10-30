/** The Admin screens' queries and writes, over `data/admin-api.ts`, with the web's keys and refreshes. */
import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { accountKeys } from '../data/account-api'
import * as admin from '../data/admin-api'
import { useOrbit } from '../orbit-context'

export function useAdminUsers(search: string) {
  const { client } = useOrbit()
  return useInfiniteQuery({
    queryKey: admin.adminKeys.users(search),
    queryFn: ({ pageParam }) => admin.adminUsers(client, search, pageParam),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
    // the list stays while the next search loads
    placeholderData: keepPreviousData,
  })
}

export function useAuditEvents() {
  const { client } = useOrbit()
  return useInfiniteQuery({
    queryKey: admin.adminKeys.audit,
    queryFn: ({ pageParam }) => admin.auditEvents(client, pageParam),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
  })
}

export function useBackups() {
  const { client } = useOrbit()
  return useQuery({ queryKey: admin.adminKeys.backups, queryFn: () => admin.backups(client) })
}

/** Create and delete a backup; both refresh the list. */
export function useBackupWrites() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  const onSettled = () => void queryClient.invalidateQueries({ queryKey: admin.adminKeys.backups })
  return {
    create: useMutation({ mutationFn: () => admin.createBackup(client), onSettled }),
    remove: useMutation({ mutationFn: (backupId: string) => admin.deleteBackup(client, backupId), onSettled }),
  }
}

export function useJobLimits() {
  const { client } = useOrbit()
  return useQuery({ queryKey: admin.adminKeys.jobs, queryFn: () => admin.jobLimits(client) })
}

export function useSaveJobLimits() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: Parameters<typeof admin.saveJobLimits>[1]) => admin.saveJobLimits(client, body),
    onSuccess: (kinds) => queryClient.setQueryData(admin.adminKeys.jobs, kinds),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: admin.adminKeys.audit }),
  })
}

/** The Users menu's writes. Each refreshes every users list and the audit log. */
export function useAdminUserWrites() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  const audit = () => void queryClient.invalidateQueries({ queryKey: admin.adminKeys.audit })
  const onSettled = () => {
    void queryClient.invalidateQueries({ queryKey: ['admin', 'users'] })
    audit()
  }
  return {
    suspend: useMutation({ mutationFn: ({ userId, suspended }: { userId: string; suspended: boolean }) => admin.setSuspended(client, userId, suspended), onSettled }),
    setAdmin: useMutation({ mutationFn: ({ userId, isAdmin }: { userId: string; isAdmin: boolean }) => admin.setAdmin(client, userId, isAdmin), onSettled }),
    resetTwoFactor: useMutation({ mutationFn: (userId: string) => admin.clearTwoFactor(client, userId), onSettled }),
    recoveryLink: useMutation({ mutationFn: (userId: string) => admin.recoveryLink(client, userId), onSettled: audit }),
    changeEmail: useMutation({ mutationFn: ({ userId, email }: { userId: string; email: string }) => admin.changeEmail(client, userId, email), onSettled }),
  }
}

/** The instance settings of the General and Storage pages; polls while files move or images compress. */
export function useInstanceSettings() {
  const { client } = useOrbit()
  return useQuery({
    queryKey: admin.adminKeys.settings,
    queryFn: () => admin.instanceSettings(client),
    refetchInterval: (query) => admin.settingsPollInterval(query.state.data?.storage),
  })
}

/** A settings write: it answers with the whole settings, and the sign-in options follow from them. */
function useSettingsWrite<T>(write: (client: ReturnType<typeof useOrbit>['client'], input: T) => Promise<admin.InstanceSettingsView>) {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (input: T) => write(client, input),
    onSuccess: (settings) => {
      queryClient.setQueryData(admin.adminKeys.settings, settings)
      void queryClient.invalidateQueries({ queryKey: accountKeys.authOptions })
    },
  })
}

export const useSetRegistration = () => useSettingsWrite(admin.setRegistration)
export const useSaveSmtp = () => useSettingsWrite(admin.saveSmtp)
export const useRemoveSmtp = () => useSettingsWrite((client, _: void) => admin.removeSmtp(client))
export const useSaveLimits = () => useSettingsWrite(admin.saveLimits)
export const useSaveS3 = () => useSettingsWrite(admin.saveS3)
export const useRemoveS3 = () => useSettingsWrite((client, _: void) => admin.removeS3(client))
export const useSaveStorageOptions = () => useSettingsWrite(admin.saveStorageOptions)
export const useStartImageCompression = () => useSettingsWrite((client, _: void) => admin.startImageCompression(client))

export function useSendTestEmail() {
  const { client } = useOrbit()
  return useMutation({ mutationFn: () => admin.sendTestEmail(client) })
}
