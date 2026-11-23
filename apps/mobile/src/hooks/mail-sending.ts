/** The queries and writes of Settings → Mail → Sending and Mail preferences, over `data/mail-sending-api.ts`, with the web's refreshes. */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { mailKeys } from '../data/keys'
import * as api from '../data/mail-sending-api'
import { mailSendingKeys as keys, type ProviderScope } from '../data/mail-sending-api'
import { useOrbit } from '../orbit-context'

/** The From addresses of every thread: the composer reads each address's signature, whether it can send and its size limit there. */
const fromAddressesPrefix = (workspaceId: string) => mailKeys.fromAddresses(workspaceId).slice(0, 3)
/** The web's `mailKeys.domains`: a domain shows its route and that route's SPF. */
const domainsPrefix = (workspaceId: string) => ['mail', workspaceId, 'domains'] as const

export function useSendRoutes(scope: ProviderScope) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.sendRoutes(scope, workspaceId), queryFn: () => api.sendRoutes(client, scope, workspaceId) })
}

/** After a route changes: the route list, and in the current workspace its routes, domains and From addresses. */
function useSendRouteRefresh(scope: ProviderScope) {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return () => {
    void queryClient.invalidateQueries({ queryKey: keys.sendRoutes(scope, workspaceId) })
    if (scope === 'instance') void queryClient.invalidateQueries({ queryKey: keys.sendRoutes('workspace', workspaceId) })
    void queryClient.invalidateQueries({ queryKey: domainsPrefix(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: fromAddressesPrefix(workspaceId) })
  }
}

export function useSendRouteWrites(scope: ProviderScope) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSettled = useSendRouteRefresh(scope)
  return {
    create: useMutation({ mutationFn: (body: api.CreateMailSendRouteBody) => api.createSendRoute(client, scope, workspaceId, body), onSettled }),
    update: useMutation({ mutationFn: ({ routeId, body }: { routeId: string; body: api.UpdateMailSendRouteBody }) => api.updateSendRoute(client, scope, workspaceId, routeId, body), onSettled }),
    remove: useMutation({ mutationFn: (routeId: string) => api.deleteSendRoute(client, scope, workspaceId, routeId), onSettled }),
    createFromProvider: useMutation({
      mutationFn: ({ providerId, body }: { providerId: string; body: api.CreateProviderSendRouteBody }) => api.createProviderSendRoute(client, scope, workspaceId, providerId, body),
      onSettled,
    }),
    // the route's last send or error changed
    test: useMutation({
      mutationFn: ({ routeId, to }: { routeId: string; to: string }) => api.testSendRoute(client, scope, workspaceId, routeId, to),
      onSettled: () => queryClient.invalidateQueries({ queryKey: keys.sendRoutes(scope, workspaceId) }),
    }),
  }
}

/** The instance default route (root user); the current workspace's domains that follow it now send through another. */
export function useSetSendDefault() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (routeId: string | null) => api.setSendDefault(client, routeId),
    onSuccess: (list) => queryClient.setQueryData(keys.sendRoutes('instance', workspaceId), list),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: keys.sendRoutes('workspace', workspaceId) })
      void queryClient.invalidateQueries({ queryKey: domainsPrefix(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: fromAddressesPrefix(workspaceId) })
    },
  })
}

// ---- Mail preferences

export function useSignatures(addressIds: readonly string[]) {
  const { client, workspaceId } = useOrbit()
  return useQuery({
    queryKey: [...keys.signatures(workspaceId), ...addressIds],
    queryFn: () => api.signatures(client, workspaceId, addressIds),
    enabled: addressIds.length > 0,
  })
}

export function useSaveSignature() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ addressId, bodyHtml }: { addressId: string; bodyHtml: string }) => api.saveSignature(client, workspaceId, addressId, bodyHtml),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: keys.signatures(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: fromAddressesPrefix(workspaceId) })
    },
  })
}

export function useMailSettingsMe() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.settingsMe(workspaceId), queryFn: () => api.mailSettings(client, workspaceId) })
}

export function useSaveMailSettings() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (undoSendSeconds: number) => api.saveMailSettings(client, workspaceId, undoSendSeconds),
    // the server holds each sent message for this time: the mail app caches nothing of it
    onSuccess: (settings) => queryClient.setQueryData(keys.settingsMe(workspaceId), settings),
  })
}

export function useSnippets() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.snippets(workspaceId), queryFn: () => api.snippets(client, workspaceId) })
}

export function useSnippetWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSettled = () => queryClient.invalidateQueries({ queryKey: keys.snippets(workspaceId) })
  return {
    create: useMutation({ mutationFn: (body: { name: string; body_html: string }) => api.createSnippet(client, workspaceId, body), onSettled }),
    update: useMutation({ mutationFn: ({ snippetId, body }: { snippetId: string; body: { name: string; body_html: string } }) => api.updateSnippet(client, workspaceId, snippetId, body), onSettled }),
    remove: useMutation({ mutationFn: (snippetId: string) => api.deleteSnippet(client, workspaceId, snippetId), onSettled }),
  }
}

export function useVacation(mailboxId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.vacation(workspaceId, mailboxId), queryFn: () => api.vacation(client, workspaceId, mailboxId), enabled: mailboxId !== '' })
}

export function useSetVacation() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ mailboxId, body }: { mailboxId: string; body: api.PutMailVacationBody }) => api.setVacation(client, workspaceId, mailboxId, body),
    onSuccess: (saved, { mailboxId }) => queryClient.setQueryData(keys.vacation(workspaceId, mailboxId), saved),
  })
}
