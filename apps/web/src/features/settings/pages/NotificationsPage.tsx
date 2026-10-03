import { toast } from 'sonner'
import { Monitor, Mobile as Smartphone } from 'reicon-react'
import type { NotificationPrefs } from '@/api/generated/types.gen'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { AWAY_MINUTES, useAwayMinutes } from '@/features/realtime/idle'
import { needsHomeScreen, pushSupported } from '@/features/realtime/push'
import {
  useDisablePush,
  useEnablePush,
  useNotificationPreferences,
  usePushDevices,
  useRemovePushDevice,
  useSaveNotificationPreferences,
  useSendTestPush,
  useThisDeviceEndpoint,
} from '@/features/settings/api/notifications'
import { FieldGrid, RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { shortDate } from '@/lib/format'
import { MIN_VOLUME, playSound, SOUND_SETS, type SoundSet, useSoundSettings } from '@/lib/sounds'

const PREFERENCES: { field: keyof NotificationPrefs; label: string }[] = [
  { field: 'direct_messages', label: 'Direct messages' },
  { field: 'chat_mentions', label: 'Mentions in chat' },
  { field: 'thread_replies', label: 'Replies in threads I follow' },
  { field: 'channel_messages', label: 'All messages in channels set to "All messages"' },
  { field: 'task_assigned', label: 'Tasks assigned to me' },
  { field: 'mentions', label: 'Mentions in comments and pages' },
]

/** Whether this browser gets notifications, with the button that changes it. */
function ThisDevice() {
  const devices = usePushDevices()
  const endpoint = useThisDeviceEndpoint()
  const enable = useEnablePush()
  const disable = useDisablePush()
  const test = useSendTestPush()

  if (needsHomeScreen(navigator.userAgent, (navigator as { standalone?: boolean }).standalone, navigator.maxTouchPoints)) {
    return <p className="text-sm text-muted-foreground">Add Orbit to your Home Screen to get notifications: in Safari, tap Share, then "Add to Home Screen", and open Orbit from there.</p>
  }
  if (!pushSupported()) return <p className="text-sm text-muted-foreground">This browser does not support notifications.</p>
  if (Notification.permission === 'denied') {
    return (
      <p className="text-sm text-muted-foreground">
        Notifications are blocked for Orbit in this browser. Allow them in the site settings (the icon beside the address bar), then reload this page.
      </p>
    )
  }
  if (endpoint.isPending || devices.isPending) return <p className="text-sm text-muted-foreground" role="status">Checking this device…</p>

  // On only when the server knows this browser too: a device removed elsewhere is off here.
  const on = endpoint.data != null && (devices.data ?? []).some((device) => device.endpoint === endpoint.data)
  const busy = enable.isPending || disable.isPending

  const turnOn = () =>
    enable.mutate(undefined, {
      onSuccess: (result) => {
        if (result === 'dismissed') toast('Notifications stay off until you allow them.')
      },
      onError: () => toast.error('Could not turn on notifications. Try again.'),
    })
  const turnOff = () => disable.mutate(undefined, { onError: () => toast.error('Could not turn off notifications. Try again.') })
  const sendTest = () =>
    test.mutate(undefined, {
      onSuccess: (sent) => toast(`Sent to ${sent} ${sent === 1 ? 'device' : 'devices'}`),
      onError: () => toast.error('Could not send the test notification. Try again.'),
    })

  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <p className="text-sm text-muted-foreground">{on ? 'Notifications are on for this device.' : 'Notifications are off on this device.'}</p>
      <div className="flex flex-wrap items-center gap-2">
        {on ? (
          <>
            <Button type="button" variant="outline" disabled={test.isPending} onClick={sendTest}>Send a test notification</Button>
            <Button type="button" variant="outline" disabled={busy} onClick={turnOff}>Turn off</Button>
          </>
        ) : (
          <Button type="button" disabled={busy} onClick={turnOn}>Turn on notifications</Button>
        )}
      </div>
    </div>
  )
}

function Devices() {
  const devices = usePushDevices()
  const endpoint = useThisDeviceEndpoint()
  const remove = useRemovePushDevice()
  const list = devices.data ?? []

  return (
    <div className="flex flex-col divide-y">
      {devices.isPending ? <SettingsRow role="status">Loading devices…</SettingsRow> : null}
      {devices.isError ? <SettingsRow role="alert">Devices could not be loaded. <Button variant="ghost" onClick={() => void devices.refetch()}>Retry</Button></SettingsRow> : null}
      {devices.isSuccess && list.length === 0 ? <SettingsRow className="text-sm text-muted-foreground">No device gets notifications yet.</SettingsRow> : null}
      {list.map((device) => (
        <SettingsRow key={device.id} className="flex-nowrap">
          <RowIcon>{/Android|iOS|iPadOS/.test(device.label) ? <Smartphone /> : <Monitor />}</RowIcon>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="flex items-center gap-2 text-[13px] font-medium text-foreground">
              <span className="truncate">{device.label}</span>
              {device.endpoint === endpoint.data ? <Badge>This device</Badge> : null}
            </span>
            <span className="text-xs text-muted-foreground/70">Added {shortDate(device.created_at)}</span>
          </div>
          <Button
            type="button"
            variant="ghost"
            disabled={remove.isPending}
            onClick={() => remove.mutate(device, { onError: () => toast.error(`Could not remove ${device.label}. Try again.`) })}
          >
            Remove
          </Button>
        </SettingsRow>
      ))}
    </div>
  )
}

function Preferences() {
  const preferences = useNotificationPreferences()
  const save = useSaveNotificationPreferences()
  const current = preferences.data

  if (preferences.isPending) return <p className="text-sm text-muted-foreground" role="status">Loading…</p>
  if (!current) return <p className="text-sm text-muted-foreground" role="alert">Your choices could not be loaded. <Button variant="ghost" onClick={() => void preferences.refetch()}>Retry</Button></p>

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col divide-y divide-border">
        {PREFERENCES.map(({ field, label }) => (
          <div key={field} className="flex items-center justify-between gap-4 py-3 first:pt-0 last:pb-0">
            <span className="text-sm font-medium text-foreground">{label}</span>
            <Switch
              aria-label={label}
              checked={current[field]}
              onCheckedChange={(checked: boolean) =>
                save.mutate({ ...current, [field]: checked }, { onError: () => toast.error('Could not save your choice. Try again.') })
              }
            />
          </div>
        ))}
      </div>
      <p className="text-xs text-muted-foreground">Do not disturb turns all of these off. Muted channels never notify.</p>
    </div>
  )
}

const AWAY_OPTIONS = AWAY_MINUTES.map((minutes) => ({ value: String(minutes), label: minutes === 1 ? '1 minute' : `${minutes} minutes` }))

function OtherDevices() {
  const [minutes, setMinutes] = useAwayMinutes()
  return (
    <FieldGrid>
      <Field>
        <FieldLabel htmlFor="away-minutes">Notify my other devices when this window is out of focus for</FieldLabel>
        <Select items={AWAY_OPTIONS} value={String(minutes)} onValueChange={(value) => setMinutes(Number(value))}>
          <SelectTrigger id="away-minutes">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {AWAY_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
          </SelectContent>
        </Select>
        <FieldDescription>Before that, only this device notifies you. A closed or idle Orbit always notifies your other devices.</FieldDescription>
      </Field>
    </FieldGrid>
  )
}

function Sounds() {
  const [settings, setSettings] = useSoundSettings()
  // A preview plays with the sound switch off too: the user is choosing what it would sound like.
  const preview = (kind: 'message' | 'mention') => playSound(kind, { ...settings, enabled: true })

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-4">
        <span className="text-sm font-medium text-foreground">Play a sound</span>
        <Switch aria-label="Play a sound" checked={settings.enabled} onCheckedChange={(enabled: boolean) => setSettings({ enabled })} />
      </div>
      <FieldGrid>
        <Field>
          <FieldLabel htmlFor="sound-set">Sound</FieldLabel>
          <Select items={SOUND_SETS} value={settings.set} onValueChange={(value) => setSettings({ set: value as SoundSet })}>
            <SelectTrigger id="sound-set">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SOUND_SETS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </Field>
        <Field>
          <FieldLabel htmlFor="sound-volume">Volume</FieldLabel>
          <input
            id="sound-volume"
            type="range"
            className="h-8 w-full accent-primary"
            min={MIN_VOLUME}
            max={1}
            step={0.05}
            value={settings.volume}
            onChange={(event) => setSettings({ volume: Number(event.target.value) })}
          />
        </Field>
      </FieldGrid>
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm text-muted-foreground">Preview</span>
        <Button type="button" variant="outline" onClick={() => preview('message')}>Message</Button>
        <Button type="button" variant="outline" onClick={() => preview('mention')}>Mention</Button>
      </div>
    </div>
  )
}

/** Account view: where and about what Orbit notifies the user. */
export function NotificationsPage() {
  return (
    <>
      <SettingsCard title="This device" description="Get a notification here when you are not looking at Orbit.">
        <ThisDevice />
      </SettingsCard>
      <SettingsCard title="Devices" description="Every browser and device that gets your notifications." flush>
        <Devices />
      </SettingsCard>
      <SettingsCard title="Other devices" description="When your phone and other browsers get a notification that this device already shows.">
        <OtherDevices />
      </SettingsCard>
      <SettingsCard title="Notify me about" description="These apply to all your devices.">
        <Preferences />
      </SettingsCard>
      <SettingsCard title="Sounds" description="The sound of a new message or mention on this device.">
        <Sounds />
      </SettingsCard>
    </>
  )
}
