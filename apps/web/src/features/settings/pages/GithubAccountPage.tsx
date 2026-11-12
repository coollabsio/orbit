import { useEffect, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import { ProviderMark } from '@/components/common/icons/ProviderMark'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { type FinishResult, useConnectGithub, useDisconnectGithub, useFinishGithub, useGithubConnection } from '@/features/github/api'
import { ConnectGithubPrompt } from '@/features/github/components/ConnectGithubPrompt'
import { RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { loadFailed } from '@/lib/connection'

const RESULTS: Record<FinishResult, { ok: boolean; text: string }> = {
  connected: { ok: true, text: 'GitHub connected' },
  failed: { ok: false, text: 'Could not connect GitHub. Try again.' },
  taken: { ok: false, text: 'This GitHub account is already connected to another Orbit user.' },
  mismatch: { ok: false, text: 'Another member started this GitHub connection. Connect from your own Account settings.' },
}

function showResult(result: FinishResult) {
  const shown = RESULTS[result]
  // The id keeps it to one toast when an effect runs again (StrictMode).
  ;(shown.ok ? toast.success : toast.error)(shown.text, { id: 'github-connect' })
}

/** Account settings → GitHub: the member's own GitHub account, used to review pull requests as themselves. */
export function GithubAccountPage() {
  const { workspace } = useWorkspace()
  const query = useGithubConnection(workspace.id)
  const connect = useConnectGithub(workspace.id)
  const disconnect = useDisconnectGithub(workspace.id)
  const [params, setParams] = useSearchParams()
  // the flow finishes in the workspace it started in (the member may have switched workspace on the way)
  const [finishIn] = useState(() => params.get('workspace') ?? workspace.id)
  const finish = useFinishGithub(finishIn)
  const finishOnce = finish.mutate
  const result = params.get('github')
  const token = params.get('token')
  const finished = useRef<string | null>(null)

  useEffect(() => {
    if (!result) return
    // GitHub sent the browser back: connect as the signed-in member, once for each token.
    if (result === 'finish' && token && finished.current !== token) {
      finished.current = token
      finishOnce(token, { onSuccess: showResult })
    } else if (result === 'failed') showResult('failed')
    setParams({}, { replace: true })
  }, [result, token, setParams, finishOnce])

  const description = `Review, comment on and merge pull requests in ${workspace.name} as yourself. GitHub shows your name, and your own GitHub permissions apply.`
  if (loadFailed(query)) return <SettingsCard title="GitHub" description={description}>Your GitHub connection could not be loaded.</SettingsCard>
  if (query.data === undefined) return <SettingsCard title="GitHub" description={description}>Loading your GitHub connection.</SettingsCard>
  const { status, login } = query.data
  if (status === 'unavailable' || status === 'needs_recreate') {
    return <SettingsCard title="GitHub" description={description}><ConnectGithubPrompt workspaceId={workspace.id} /></SettingsCard>
  }

  async function confirmDisconnect() {
    const confirmed = await confirmAction({
      title: 'Disconnect GitHub?',
      description: 'You can no longer review pull requests in Orbit until you connect again.',
      confirmLabel: 'Disconnect',
      danger: true,
    })
    if (confirmed) disconnect.mutate(undefined, { onSuccess: () => toast('GitHub is disconnected') })
  }

  return <SettingsCard title="GitHub" description={description} flush>
    <SettingsRow className="flex-nowrap">
      <RowIcon className="text-foreground"><ProviderMark provider="github" /></RowIcon>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-[13px] font-medium text-foreground">{login ?? 'GitHub'}</span>
        <span className="text-xs text-muted-foreground/70">{status === 'connected' ? 'Connected' : status === 'broken' ? 'GitHub no longer accepts this connection' : 'Not connected'}</span>
      </div>
      {status === 'connected'
        ? <Button variant="ghost" aria-label="Disconnect GitHub" disabled={disconnect.isPending} onClick={() => void confirmDisconnect()}>Disconnect</Button>
        : <Button variant="outline" disabled={connect.isPending} onClick={() => connect.mutate()}>{status === 'broken' ? 'Reconnect GitHub' : 'Connect GitHub'}</Button>}
    </SettingsRow>
    {connect.isError || disconnect.isError ? <SettingsRow role="alert" className="text-[13px] text-destructive">{connect.isError ? 'The connection could not start. Try again.' : 'GitHub could not be disconnected.'}</SettingsRow> : null}
  </SettingsCard>
}
