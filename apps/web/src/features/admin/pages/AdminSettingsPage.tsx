import { toast } from 'sonner'
import { useState, type FormEvent } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { ApiProblem } from '@orbit/api/problem'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useInstanceSettings, useSetCustomPagePolicy, useSetRegistration } from '@/features/admin/api'
import type { CustomPageMode, CustomPagePolicy } from '@orbit/api/generated/types.gen'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Label } from '@/components/ui/label'
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
    <CustomPagesCard policy={settings.data.custom_pages} locked={settings.data.environment.frame_origins} />
  </>
}

const MODES: { value: CustomPageMode; label: string; help: string }[] = [
  { value: 'any', label: 'Any https site', help: 'Workspace owners and admins can add pages from any https site. Each user\'s browser allows only the sites of the pages that this user sees.' },
  { value: 'listed', label: 'Only these sites', help: 'Pages must be on one of the origins below. A page whose origin you remove stays, but Orbit does not show it.' },
  { value: 'off', label: 'Off', help: 'No custom pages show, and none can be added. The pages stay for later.' },
]

function CustomPagesCard({ policy, locked }: { policy: CustomPagePolicy; locked: boolean }) {
  const queryClient = useQueryClient()
  const setPolicy = useSetCustomPagePolicy()
  const saved = policy.origins.join('\n')
  const [mode, setMode] = useState(policy.mode)
  const [text, setText] = useState(saved)
  const entries = text.split(/[\s,]+/).filter(Boolean)
  const changed = mode !== policy.mode || entries.join('\n') !== saved
  const busy = locked || setPolicy.isPending

  function submit(event: FormEvent) {
    event.preventDefault()
    setPolicy.mutate({ mode, origins: entries }, {
      onSuccess: (settings) => {
        if (settings) {
          setMode(settings.custom_pages.mode)
          setText(settings.custom_pages.origins.join('\n'))
        }
        // the custom page lists carry the policy too
        void queryClient.invalidateQueries({ predicate: (query) => query.queryKey.includes('custom-pages') })
        toast('Custom page settings saved')
      },
      onError: (error) => toast.error(problemText(error, 'The settings could not be saved.')),
    })
  }

  return (
    <SettingsCard title="Custom pages" description="Workspace owners and admins can show web pages in their sidebar (Settings → Custom pages). Orbit frames them in a sandbox. Each site must allow Orbit in its frame-ancestors.">
      <form className="grid gap-3" onSubmit={submit}>
        <RadioGroup aria-label="Which sites" value={mode} disabled={busy} onValueChange={(value) => setMode(value as CustomPageMode)}>
          {MODES.map((item) => (
            <div key={item.value} className="flex items-center gap-2">
              <Label className="flex items-center gap-2 font-normal"><RadioGroupItem value={item.value} /> {item.label}</Label>
              <InfoTip text={item.help} />
            </div>
          ))}
        </RadioGroup>
        {mode === 'listed' ? (
          <Textarea
            aria-label="Allowed origins"
            value={text}
            rows={Math.min(8, Math.max(3, entries.length + 1))}
            disabled={busy}
            placeholder={'https://grafana.example.com\nhttps://deploys.example.com'}
            className="font-mono text-xs"
            onChange={(event) => setText(event.target.value)}
          />
        ) : null}
        {locked ? <SetByEnvironment variable="ORBIT_FRAME_ORIGINS" /> : (
          <div className="flex items-center gap-3">
            <span className="flex-1 text-xs text-muted-foreground">{mode === 'listed' ? 'One https origin per line, without a path.' : null}</span>
            <Button type="submit" disabled={!changed || setPolicy.isPending}>Save</Button>
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
