/** Settings → Mail → Domains and Providers: queries and writes over `data/mail-domains-api.ts`, with the web's refreshes. */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import * as api from '../data/mail-domains-api'
import { useOrbit } from '../orbit-context'

const keys = api.mailDomainKeys

export function useMailDomains(enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.domains(workspaceId), queryFn: () => api.domains(client, workspaceId), enabled })
}

/** One domain with its records; `refetchInterval` (of the cached domain and when it was read) polls while a DNS check runs. */
export function useMailDomain(domainId: string | null, refetchInterval: (data: api.MailDomain | undefined, updatedAt: number) => number | false) {
  const { client, workspaceId } = useOrbit()
  return useQuery({
    queryKey: keys.domain(workspaceId, domainId ?? ''),
    queryFn: () => api.domain(client, workspaceId, domainId ?? ''),
    enabled: domainId !== null,
    refetchInterval: (query) => refetchInterval(query.state.data, query.state.dataUpdatedAt),
  })
}

export function useMailboxList(enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.mailboxes(workspaceId), queryFn: () => api.mailboxes(client, workspaceId), enabled })
}

export function useMailProviders(scope: api.ProviderScope, enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: scope === 'instance' ? keys.adminProviders : keys.providers(workspaceId), queryFn: () => api.providers(client, workspaceId, scope), enabled })
}

export function useMailSendRoutes(scope: api.ProviderScope, enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: scope === 'instance' ? keys.adminSendRoutes : keys.sendRoutes(workspaceId), queryFn: () => api.sendRoutes(client, workspaceId, scope), enabled })
}

/** The domain writes, each with the web's refreshes. */
export function useDomainWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const refresh = (...queryKeys: readonly (readonly string[])[]) => () => void Promise.all(queryKeys.map((queryKey) => queryClient.invalidateQueries({ queryKey })))
  const domainsKey = keys.domains(workspaceId)
  return {
    create: useMutation({
      mutationFn: ({ name, providerId }: { name: string; providerId: string | null }) => api.createDomain(client, workspaceId, name, providerId),
      onSuccess: refresh(domainsKey),
    }),
    check: useMutation({ mutationFn: (domainId: string) => api.checkDomain(client, workspaceId, domainId) }),
    remove: useMutation({ mutationFn: (domainId: string) => api.deleteDomain(client, workspaceId, domainId), onSettled: refresh(domainsKey) }),
    setCatchAll: useMutation({
      mutationFn: ({ domainId, mailboxId }: { domainId: string; mailboxId: string | null }) => api.setCatchAll(client, workspaceId, domainId, mailboxId),
      onSettled: refresh(domainsKey),
    }),
    setRoute: useMutation({
      mutationFn: ({ domainId, providerId }: { domainId: string; providerId: string | null }) => api.setDomainRoute(client, workspaceId, domainId, providerId),
      onSettled: refresh(domainsKey, keys.providers(workspaceId)),
    }),
    setSendRoute: useMutation({
      mutationFn: ({ domainId, routeId }: { domainId: string; routeId: string | null }) => api.setDomainSendRoute(client, workspaceId, domainId, routeId),
      onSettled: refresh(domainsKey, keys.sendRoutes(workspaceId), keys.fromAddresses(workspaceId)),
    }),
    setEnabled: useMutation({
      mutationFn: ({ domainId, enabled }: { domainId: string; enabled: boolean }) => api.setDomainEnabled(client, workspaceId, domainId, enabled),
      onSettled: refresh(domainsKey),
    }),
    rotate: useMutation({
      mutationFn: (domainId: string) => api.rotateDkim(client, workspaceId, domainId),
      onSuccess: (domain) => queryClient.setQueryData(keys.domain(workspaceId, domain.id), domain),
      onSettled: () => void queryClient.invalidateQueries({ queryKey: domainsKey, exact: true }),
    }),
  }
}

/** Provider writes refresh the scope's list and all of the workspace's mail (it lists the instance providers, and domains show theirs). */
export function useProviderWrites(scope: api.ProviderScope) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSettled = () => {
    void queryClient.invalidateQueries({ queryKey: scope === 'instance' ? keys.adminProviders : keys.providers(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: keys.all(workspaceId) })
  }
  return {
    create: useMutation({ mutationFn: (body: api.CreateMailProviderBody) => api.createProvider(client, workspaceId, scope, body), onSettled }),
    update: useMutation({
      mutationFn: ({ providerId, body }: { providerId: string; body: api.UpdateMailProviderBody }) => api.updateProvider(client, workspaceId, scope, providerId, body),
      onSettled,
    }),
    remove: useMutation({ mutationFn: (providerId: string) => api.deleteProvider(client, workspaceId, scope, providerId), onSettled }),
  }
}
