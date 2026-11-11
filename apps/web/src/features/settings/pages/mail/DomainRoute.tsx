import { toast } from 'sonner'
import type { MailDomain, MailProviderView } from '@orbit/api/generated/types.gen'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useSetDomainRoute } from '@/features/mail/api/mutations'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { DIRECT, mailProblem, routeOptions } from './mailSettingsLib'

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
      <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
        <span className="font-medium text-foreground">Receive mail</span>
        <span className="text-xs text-muted-foreground">Where mail for {domain.name} arrives. The MX record below changes with it.</span>
      </div>
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
