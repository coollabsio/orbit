import { Download } from 'reicon-react'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { useGlobalAudit } from '@/features/admin/api'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { LoadingText } from '@/components/ui/spinner'

/** Security events of every workspace and of the instance, newest first. */
export function AdminAuditPage() {
  const audit = useGlobalAudit()
  return (
    <SettingsCard
      title="Audit log"
      description="Sign-ins, account changes and other security events from every workspace."
      actions={
        <a href="/api/v1/admin/audit/export" download="orbit-audit.csv" className={buttonVariants({ variant: 'outline' })}>
          <Download className="size-4" />
          Export CSV
        </a>
      }
      flush
    >
      <div className="flex flex-col divide-y text-[0.8125rem]">
        {audit.isPending ? <SettingsRow><LoadingText>Loading events…</LoadingText></SettingsRow> : null}
        {audit.isError ? <SettingsRow role="alert">Events could not be loaded. <Button variant="ghost" onClick={() => void audit.refetch()}>Retry</Button></SettingsRow> : null}
        {audit.data?.length === 0 ? <SettingsRow className="text-muted-foreground">No events yet.</SettingsRow> : null}
        {audit.data?.map((event) => {
          const occurred = new Date(event.occurred_at)
          return (
            <SettingsRow key={event.id} className="flex-nowrap">
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="flex min-w-0 items-center gap-2 font-medium text-foreground">
                  <span className="truncate font-mono text-xs">{event.action}</span>
                  {event.outcome === 'failure' ? <Badge variant="destructive">Failed</Badge> : null}
                </span>
                <span className="truncate text-xs text-muted-foreground">
                  {event.actor_id ? (event.actor_name ?? 'Unknown account') : 'System'}
                </span>
              </div>
              <time dateTime={event.occurred_at} className="shrink-0 text-xs text-muted-foreground/70">{occurred.toLocaleString()}</time>
            </SettingsRow>
          )
        })}
        {audit.hasNextPage ? (
          <SettingsRow className="justify-center">
            <Button variant="ghost" disabled={audit.isFetchingNextPage} onClick={() => void audit.fetchNextPage()}>
              {audit.isFetchingNextPage ? 'Loading…' : 'Load older events'}
            </Button>
          </SettingsRow>
        ) : null}
      </div>
    </SettingsCard>
  )
}
