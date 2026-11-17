import { toast } from 'sonner'
import type { MailDomain, MailProviderView, MailSendRouteList } from '@orbit/api/generated/types.gen'
import { InfoTip } from '@/components/common/InfoTip'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useSetDomainRoute, useSetDomainSendRoute } from '@/features/mail/api/mutations'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { DIRECT, mailProblem, routeLabel, routeOptions } from './mailSettingsLib'

/** The select of where mail arrives: this server, or a provider. */
export function RouteSelect({
  id,
  label,
  value,
  providers,
  disabled,
  onChange,
}: {
  id?: string
  label: string
  value: string
  providers: MailProviderView[]
  disabled?: boolean
  onChange: (value: string) => void
}) {
  const options = routeOptions(providers)
  return (
    <Select items={options} value={value} disabled={disabled} onValueChange={(next) => onChange(next as string)}>
      <SelectTrigger id={id} aria-label={label} className="w-64 max-w-[45vw]"><SelectValue /></SelectTrigger>
      <SelectContent>
        {options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
      </SelectContent>
    </Select>
  )
}

/** A domain's row: where its mail arrives. A change makes the MX record above expect the provider's hosts. */
export function DomainRouteRow({ domain, providers }: { domain: MailDomain; providers: MailProviderView[] }) {
  const setRoute = useSetDomainRoute()
  return (
    <SettingsRow className="flex-nowrap">
      <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
        Receive mail
        <InfoTip text={`Where mail for ${domain.name} arrives. The MX record below changes with it.`} />
      </span>
      <RouteSelect
        label={`Where mail for ${domain.name} arrives`}
        value={domain.provider?.id ?? DIRECT}
        providers={providers}
        disabled={setRoute.isPending}
        onChange={(value) => setRoute.mutate({ domainId: domain.id, providerId: value === DIRECT ? null : value }, {
          onSuccess: () => toast('Route saved. Update the MX record if it changed.'),
          onError: (error) => toast.error(mailProblem(error, 'The route could not be saved.')),
        })}
      />
    </SettingsRow>
  )
}

/** The select value of the server's default route. */
const DEFAULT_ROUTE = 'default'

/** A domain's row: how its mail is sent. A change makes the SPF record below expect the route's servers. */
export function DomainSendRouteRow({ domain, routes }: { domain: MailDomain; routes: MailSendRouteList }) {
  const setRoute = useSetDomainSendRoute()
  const fallback = routes.routes.find((route) => route.id === routes.default_route_id)
  const options = [
    { value: DEFAULT_ROUTE, label: fallback ? `Server default: ${fallback.name}` : 'Server default (none set)' },
    ...routes.routes.map((route) => ({ value: route.id, label: `${route.name} (${routeLabel(route.kind)})` })),
  ]
  // `send_route` is the route in use: the domain's own, else the default one
  const current = domain.send_route?.id
  const value = !current || current === routes.default_route_id ? DEFAULT_ROUTE : current
  return (
    <SettingsRow className="flex-nowrap">
      <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
        Send mail
        <InfoTip text={`How mail from ${domain.name} is sent. The SPF record below changes with it.`} />
      </span>
      <Select
        items={options}
        value={value}
        disabled={setRoute.isPending}
        onValueChange={(next) => setRoute.mutate({ domainId: domain.id, routeId: next === DEFAULT_ROUTE ? null : (next as string) }, {
          onSuccess: () => toast('Send route saved. Update the SPF record if it changed.'),
          onError: (error) => toast.error(mailProblem(error, 'The send route could not be saved.')),
        })}
      >
        <SelectTrigger aria-label={`How mail from ${domain.name} is sent`} className="w-64 max-w-[45vw]"><SelectValue /></SelectTrigger>
        <SelectContent>
          {options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
        </SelectContent>
      </Select>
    </SettingsRow>
  )
}
