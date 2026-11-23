/** The web's `DomainRoute.tsx`: where a domain's mail arrives, and how it is sent. */
import type { ReactNode } from 'react'
import { View } from 'react-native'

import type { MailDomain, MailProviderView, MailSendRouteList } from '../../../data/mail-domains-api'
import { DIRECT, mailProblem, routeLabel, routeOptions } from '../../../data/mail-settings'
import { useDomainWrites } from '../../../hooks/mail-domains'
import { LabelTip } from '../../../ui'
import { Select } from '../Shell'

/** A message for the card (the web's toasts). */
export type Notify = (text: string, error?: boolean) => void

/** The select of where mail arrives: this server, or a provider. */
export function RouteSelect({ label, value, providers, disabled, onChange }: { label: string; value: string; providers: MailProviderView[]; disabled?: boolean; onChange: (value: string) => void }) {
  return <Select label={label} value={value} options={routeOptions(providers)} disabled={disabled} onChange={onChange} />
}

/** A label with its help above a select (a phone row has no room for both side by side). */
function RouteRow({ label, help, children }: { label: string; help: string; children: ReactNode }) {
  return (
    <View className="gap-1.5">
      <LabelTip label={label} text={help} className="text-[12px] text-muted-foreground" />
      {children}
    </View>
  )
}

/** A domain's row: where its mail arrives. A change makes the MX record below expect the provider's hosts. */
export function DomainRouteRow({ domain, providers, notify }: { domain: MailDomain; providers: MailProviderView[]; notify: Notify }) {
  const { setRoute } = useDomainWrites()
  return (
    <RouteRow label="Receive through" help={`Where mail for ${domain.name} arrives. The MX record below changes with it.`}>
      <RouteSelect
        label={`Where mail for ${domain.name} arrives`}
        value={domain.provider?.id ?? DIRECT}
        providers={providers}
        disabled={setRoute.isPending}
        onChange={(value) =>
          setRoute.mutate(
            { domainId: domain.id, providerId: value === DIRECT ? null : value },
            {
              onSuccess: () => notify('Route saved. Update the MX record below if it changed.'),
              onError: (error) => notify(mailProblem(error, 'The route could not be saved.'), true),
            },
          )
        }
      />
    </RouteRow>
  )
}

/** The select value of the server's default route. */
const DEFAULT_ROUTE = 'default'

/** A domain's row, next to the catch-all: how its mail is sent. A change makes the SPF record expect the route's servers. */
export function DomainSendRouteRow({ domain, routes, notify }: { domain: MailDomain; routes: MailSendRouteList; notify: Notify }) {
  const { setSendRoute } = useDomainWrites()
  const fallback = routes.routes.find((route) => route.id === routes.default_route_id)
  // the server's default is a choice only when the server has one
  const options = [
    ...(fallback ? [{ value: DEFAULT_ROUTE, label: `Server default (${fallback.name})` }] : []),
    ...routes.routes.map((route) => ({ value: route.id, label: `${route.name} (${routeLabel(route.kind)})` })),
  ]
  return (
    <View className="gap-1.5 border-t border-border px-4 py-3">
      <LabelTip label="Send through" className="font-noto-medium text-[13px] text-foreground" text={`How mail from ${domain.name} is sent. The SPF record under Reach the inbox changes with it.`} />
      <Select
        label={`How mail from ${domain.name} is sent`}
        // the domain's own route; without one it follows the server's default, whatever that becomes
        value={domain.own_send_route_id ?? (fallback ? DEFAULT_ROUTE : '')}
        placeholder="Pick a send route"
        options={options}
        disabled={setSendRoute.isPending}
        onChange={(next) =>
          setSendRoute.mutate(
            { domainId: domain.id, routeId: next === DEFAULT_ROUTE ? null : next },
            {
              onSuccess: () => notify('Send route saved. Update the SPF record under Reach the inbox if it changed.'),
              onError: (error) => notify(mailProblem(error, 'The send route could not be saved.'), true),
            },
          )
        }
      />
    </View>
  )
}
