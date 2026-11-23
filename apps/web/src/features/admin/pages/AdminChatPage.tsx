import { useId, useState } from 'react'
import { toast } from 'sonner'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { PasswordInput } from '@/components/ui/password-input'
import { LoadingText } from '@/components/ui/spinner'
import { useInstanceSettings, useRemoveKlipy, useSaveKlipy } from '@/features/admin/api'
import { SetByEnvironment } from '@/features/admin/components/SetByEnvironment'

const KLIPY_PARTNER_URL = 'https://partner.klipy.com'

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** The instance's chat: the KLIPY key that lets members send GIFs and stickers. */
export function AdminChatPage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="GIFs and stickers"><p><LoadingText>Loading settings…</LoadingText></p></SettingsCard>
  if (settings.isError) return <SettingsCard title="GIFs and stickers"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  return <KlipyCard keySet={settings.data.klipy.key_set} locked={settings.data.environment.klipy} />
}

/** `locked`: `ORBIT_KLIPY_API_KEY` sets the key, so it cannot be saved or removed here. */
function KlipyCard({ keySet, locked }: { keySet: boolean; locked: boolean }) {
  const save = useSaveKlipy()
  const remove = useRemoveKlipy()
  const [apiKey, setApiKey] = useState('')
  const id = useId()

  async function removeKey() {
    const confirmed = await confirmAction({
      title: 'Remove the KLIPY key?',
      description: 'Chat members can no longer send GIFs and stickers from KLIPY. Messages that have one keep it.',
      confirmLabel: 'Remove',
      danger: true,
    })
    if (!confirmed) return
    remove.mutate(undefined, {
      onSuccess: () => toast('KLIPY key removed'),
      onError: (error) => toast.error(problemText(error, 'The KLIPY key could not be removed.')),
    })
  }

  return (
    <form
      onSubmit={(event) => {
        event.preventDefault()
        save.mutate(
          { api_key: apiKey.trim() },
          {
            onSuccess: () => {
              setApiKey('')
              toast('KLIPY key saved')
            },
            onError: (error) => toast.error(problemText(error, 'The KLIPY key could not be saved.')),
          },
        )
      }}
    >
      <SettingsCard
        title="GIFs and stickers"
        description="With a KLIPY API key, chat members can search and send GIFs and stickers from KLIPY. The images load from KLIPY's servers in each reader's browser. KLIPY is free: get a key at partner.klipy.com."
      >
        <Field>
          <FieldLabel htmlFor={`${id}-key`}>
            KLIPY API key <InfoTip text="Create an app at partner.klipy.com and copy its API key. Orbit stores it encrypted and never shows it again." />
          </FieldLabel>
          <PasswordInput
            id={`${id}-key`}
            autoComplete="new-password"
            spellCheck={false}
            required
            maxLength={200}
            placeholder={keySet ? 'Saved. Enter a new key to replace it.' : ''}
            disabled={locked}
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
          />
        </Field>
        <div className="mt-4 flex flex-wrap items-center justify-end gap-2">
          {locked ? (
            <SetByEnvironment variable="ORBIT_KLIPY_API_KEY" className="mr-auto" />
          ) : (
            <p className="mr-auto text-xs text-muted-foreground">
              {keySet ? 'Key saved. GIFs and stickers are on.' : 'No key. GIFs and stickers are off.'}{' '}
              <a href={KLIPY_PARTNER_URL} target="_blank" rel="noreferrer" className="underline underline-offset-2 hover:text-foreground">
                Get a key
              </a>
            </p>
          )}
          {keySet && !locked ? (
            <Button type="button" variant="ghost" className="text-destructive" disabled={remove.isPending} onClick={() => void removeKey()}>
              Remove
            </Button>
          ) : null}
          <Button type="submit" disabled={locked || save.isPending || apiKey.trim() === ''}>
            {save.isPending ? 'Saving…' : 'Save key'}
          </Button>
        </div>
      </SettingsCard>
    </form>
  )
}
