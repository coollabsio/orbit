/** Admin → Mail and Admin → Limits → Email, over `data/admin-mail-api.ts`, with the web's keys and refreshes. */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { accountKeys } from '../data/account-api'
import { adminKeys } from '../data/admin-api'
import * as api from '../data/admin-mail-api'
import { useOrbit } from '../orbit-context'

/** The switches, the listener, the certificate and the PTR check (looked up live). */
export function useAdminMail() {
  const { client } = useOrbit()
  return useQuery({ queryKey: api.adminMailKeys.admin, queryFn: () => api.mailAdmin(client) })
}

/** The Mail switch shows or hides the Mail app, its settings and the dock item at once (`mail_enabled` in the auth options). */
export function useSetAdminMail() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: Parameters<typeof api.saveMailAdmin>[1]) => api.saveMailAdmin(client, body),
    onSuccess: (admin) => {
      queryClient.setQueryData(api.adminMailKeys.admin, admin)
      void queryClient.invalidateQueries({ queryKey: adminKeys.settings })
      void queryClient.invalidateQueries({ queryKey: accountKeys.authOptions })
    },
  })
}

/** Asked for every 3 s while an HTTP check runs (it can outlast the request that started it). */
export function useMailCertificate() {
  const { client } = useOrbit()
  return useQuery({
    queryKey: api.adminMailKeys.certificate,
    queryFn: () => api.mailCertificate(client),
    refetchInterval: (query) => (query.state.data?.pending?.method === 'http' ? api.POLL_MS : false),
  })
}

/** Request, verify or cancel the certificate. Each answers with the new state; the health reloads. */
export function useCertificateWrites() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  const onSuccess = (certificate: api.MailCertificate) => {
    queryClient.setQueryData(api.adminMailKeys.certificate, certificate)
    void queryClient.invalidateQueries({ queryKey: api.adminMailKeys.admin, exact: true })
  }
  return {
    start: useMutation({ mutationFn: (method: api.MailCertificateMethod) => api.startCertificate(client, method), onSuccess }),
    verify: useMutation({ mutationFn: () => api.verifyCertificate(client), onSuccess }),
    cancel: useMutation({ mutationFn: () => api.cancelCertificate(client), onSuccess }),
  }
}

export function useCreateTestAddress() {
  const { client } = useOrbit()
  return useMutation({ mutationFn: (domainId: string) => api.createTestAddress(client, domainId) })
}

/** The result of a test address, asked for every 3 s while it waits for the message. */
export function useTestAddress(testId: string | undefined) {
  const { client } = useOrbit()
  return useQuery({
    queryKey: api.adminMailKeys.test(testId ?? ''),
    queryFn: () => api.testAddress(client, testId!),
    enabled: Boolean(testId),
    refetchInterval: (query) => (query.state.data?.status === 'waiting' ? api.POLL_MS : false),
  })
}

export function useSendTiers() {
  const { client } = useOrbit()
  return useQuery({ queryKey: api.adminMailKeys.sendTiers, queryFn: () => api.sendTiers(client) })
}

export function useSendPolicies() {
  const { client } = useOrbit()
  return useQuery({ queryKey: api.adminMailKeys.sendPolicies, queryFn: () => api.sendPolicies(client) })
}

/** The tier limits, a workspace's tier, and Resume. Each refreshes the workspace list. */
export function useSendLimitWrites() {
  const { client } = useOrbit()
  const queryClient = useQueryClient()
  const onSettled = () => void queryClient.invalidateQueries({ queryKey: api.adminMailKeys.sendPolicies })
  return {
    tiers: useMutation({
      mutationFn: (body: api.TierLimits) => api.saveSendTiers(client, body),
      onSuccess: (tiers) => queryClient.setQueryData(api.adminMailKeys.sendTiers, tiers),
      onSettled,
    }),
    policy: useMutation({
      mutationFn: ({ workspaceId, body }: { workspaceId: string; body: Parameters<typeof api.saveSendPolicy>[2] }) => api.saveSendPolicy(client, workspaceId, body),
      onSettled,
    }),
    resume: useMutation({ mutationFn: (workspaceId: string) => api.resumeSending(client, workspaceId), onSettled }),
  }
}
