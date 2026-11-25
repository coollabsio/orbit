import { toast } from 'sonner'
import { useState, type FormEvent } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { ApiProblem } from '@orbit/api/problem'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useInstanceSettings, useSetFrameOrigins, useSetRegistration } from '@/features/admin/api'
import { Textarea } from '@/components/ui/textarea'
import { SetByEnvironment } from '@/features/admin/components/SetByEnvironment'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { LoadingText } from '@/components/ui/spinner'

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** Instance settings: who can create an account, and which sites workspaces may show as custom pages. */
export function AdminSettingsPage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="General"><p><LoadingText>Loading settings…</LoadingText></p></SettingsCard>
  if (settings.isError) return <SettingsCard title="General"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  return <>
    <RegistrationCard open={settings.data.registration_open} ready={settings.data.registration_ready} locked={settings.data.environment.registration_open} />
    <FrameOriginsCard origins={settings.data.frame_origins} locked={settings.data.environment.frame_origins} />
  </>
}

function FrameOriginsCard({ origins, locked }: { origins: string[]; locked: boolean }) {
  const queryClient = useQueryClient()
  const setFrameOrigins = useSetFrameOrigins()
  const saved = origins.join('\n')
  const [text, setText] = useState(saved)
  const entries = text.split(/[\s,]+/).filter(Boolean)
  const changed = entries.join('\n') !== saved

  function submit(event: FormEvent) {
    event.preventDefault()
    setFrameOrigins.mutate(entries, {
      onSuccess: (settings) => {
        setText((settings?.frame_origins ?? entries).join('\n'))
        // the custom page lists carry the origins too
        void queryClient.invalidateQueries({ predicate: (query) => query.queryKey.includes('custom-pages') })
        toast('Custom page origins saved')
      },
      onError: (error) => toast.error(problemText(error, 'The origins could not be saved.')),
    })
  }

  return (
    <SettingsCard title="Custom page origins" description="Workspace owners and admins can show pages from these sites in their sidebar (Settings → Custom pages). Orbit frames them in a sandbox. Each site must allow Orbit in its frame-ancestors.">
      <form className="grid gap-3" onSubmit={submit}>
        <Textarea
          aria-label="Custom page origins"
          value={text}
          rows={Math.min(8, Math.max(3, entries.length + 1))}
          disabled={locked || setFrameOrigins.isPending}
          placeholder={'https://grafana.example.com\nhttps://deploys.example.com'}
          className="font-mono text-xs"
          onChange={(event) => setText(event.target.value)}
        />
        {locked ? <SetByEnvironment variable="ORBIT_FRAME_ORIGINS" /> : (
          <div className="flex items-center gap-3">
            <span className="flex-1 text-xs text-muted-foreground">One https origin per line, without a path. At most 20.</span>
            <Button type="submit" disabled={!changed || setFrameOrigins.isPending}>Save</Button>
          </div>
        )}
      </form>
    </SettingsCard>
  )
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
