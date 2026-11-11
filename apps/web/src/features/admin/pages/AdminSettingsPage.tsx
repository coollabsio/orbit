import { toast } from 'sonner'
import { ApiProblem } from '@orbit/api/problem'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useInstanceSettings, useSetRegistration } from '@/features/admin/api'
import { SetByEnvironment } from '@/features/admin/components/SetByEnvironment'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { LoadingText } from '@/components/ui/spinner'

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** Instance settings: who can create an account. */
export function AdminSettingsPage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="General"><p><LoadingText>Loading settings…</LoadingText></p></SettingsCard>
  if (settings.isError) return <SettingsCard title="General"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  return <RegistrationCard open={settings.data.registration_open} ready={settings.data.registration_ready} locked={settings.data.environment.registration_open} />
}

function RegistrationCard({ open, ready, locked }: { open: boolean; ready: boolean; locked: boolean }) {
  const setRegistration = useSetRegistration()
  return (
    <SettingsCard title="Registration" description="Who can create an account on this Orbit." flush>
      <SettingsRow className="flex-nowrap">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
          {ready
            ? <span className="inline-flex items-center gap-1.5 font-medium text-foreground">Open registration <InfoTip text="On: anyone can create an account, with a link sent to their email or with a sign-in provider. Off: people join only by invitation." /></span>
            : <span className="font-medium text-foreground">Open registration</span>}
          {ready ? null : <span className="text-xs text-muted-foreground">Set up a mail server in Email or a sign-in provider first: it confirms the email of a new account.</span>}
          {locked ? <SetByEnvironment variable="ORBIT_OPEN_REGISTRATION" /> : null}
        </div>
        <Switch
          aria-label="Open registration"
          checked={open}
          disabled={locked || (!ready && !open) || setRegistration.isPending}
          onCheckedChange={(checked: boolean) => setRegistration.mutate(checked, {
            onSuccess: () => toast(checked ? 'Registration is open' : 'Registration is closed'),
            onError: (error) => toast.error(problemText(error, 'Registration could not be changed.')),
          })}
        />
      </SettingsRow>
    </SettingsCard>
  )
}
