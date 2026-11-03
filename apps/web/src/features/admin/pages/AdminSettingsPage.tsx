import { toast } from 'sonner'
import { ApiProblem } from '@orbit/api/problem'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useInstanceSettings, useSetRegistration } from '@/features/admin/api'
import { SettingsRow } from '@/features/settings/components/SettingsParts'

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** Instance settings: who can create an account. */
export function AdminSettingsPage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="General"><p role="status">Loading settings…</p></SettingsCard>
  if (settings.isError) return <SettingsCard title="General"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  return <RegistrationCard open={settings.data.registration_open} emailReady={settings.data.smtp != null} />
}

function RegistrationCard({ open, emailReady }: { open: boolean; emailReady: boolean }) {
  const setRegistration = useSetRegistration()
  return (
    <SettingsCard title="Registration" description="Who can create an account on this Orbit." flush>
      <SettingsRow className="flex-nowrap">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
          <span className="font-medium text-foreground">Open registration</span>
          <span className="text-xs text-muted-foreground">
            {emailReady
              ? 'On: anyone can create an account with a link sent to their email. Off: people join only by invitation.'
              : 'Set up a mail server in Email first: the sign-up link goes out by email.'}
          </span>
        </div>
        <Switch
          aria-label="Open registration"
          checked={open}
          disabled={(!emailReady && !open) || setRegistration.isPending}
          onCheckedChange={(checked: boolean) => setRegistration.mutate(checked, {
            onSuccess: () => toast(checked ? 'Registration is open' : 'Registration is closed'),
            onError: (error) => toast.error(problemText(error, 'Registration could not be changed.')),
          })}
        />
      </SettingsRow>
    </SettingsCard>
  )
}
