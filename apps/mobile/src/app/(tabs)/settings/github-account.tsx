import { useState } from 'react'
import { View } from 'react-native'

import { ConnectGithubPrompt } from '../../../components/github/ConnectGithubPrompt'
import { ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { ACCOUNT_NAV, Note, RowIcon, RowText, SettingsCard, SettingsRow, SettingsShell } from '../../../components/settings/Shell'
import { CONNECT_MESSAGES } from '../../../data/github-api'
import { useConnectGithub, useDisconnectGithub, useGithubConnection } from '../../../hooks/github'
import { useOrbit } from '../../../orbit-context'
import { Button } from '../../../ui'

/** The web's Account settings → GitHub: the member's own GitHub account, to review pull requests as themselves. */
export default function GithubAccountScreen() {
  const { workspace } = useOrbit()
  const query = useGithubConnection()
  const connect = useConnectGithub()
  const disconnect = useDisconnectGithub()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const description = `Review, comment on and merge pull requests in ${workspace.name} as yourself. GitHub shows your name, and your own GitHub permissions apply.`
  const data = query.data
  const outcome = connect.data

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV} onRefresh={() => query.refetch()}>
      <SettingsCard title="GitHub" description={description} flush={data !== undefined && data.status !== 'unavailable' && data.status !== 'needs_recreate'}>
        {query.isError ? <View className="p-4"><Note error>Your GitHub connection could not be loaded.</Note></View> : null}
        {query.isPending ? <View className="p-4"><Note loading>Loading your GitHub connection…</Note></View> : null}
        {data && (data.status === 'unavailable' || data.status === 'needs_recreate') ? <ConnectGithubPrompt status={data.status} /> : null}
        {data && data.status !== 'unavailable' && data.status !== 'needs_recreate' ? (
          <SettingsRow first>
            <RowIcon icon="link2" />
            <RowText
              title={data.login ?? 'GitHub'}
              subtitle={data.status === 'connected' ? 'Connected' : data.status === 'broken' ? 'GitHub no longer accepts this connection' : 'Not connected'}
            />
            {data.status === 'connected' ? (
              <Button
                label="Disconnect"
                variant="ghost"
                disabled={disconnect.isPending}
                onPress={() => setConfirm({ title: 'Disconnect GitHub?', description: 'You can no longer review pull requests in Orbit until you connect again.', confirmLabel: 'Disconnect', danger: true, onConfirm: () => disconnect.mutate() })}
              />
            ) : (
              <Button label={data.status === 'broken' ? 'Reconnect GitHub' : 'Connect GitHub'} variant="outline" loading={connect.isPending} onPress={() => connect.mutate()} />
            )}
          </SettingsRow>
        ) : null}
        {outcome ? <View className="px-4 pb-3"><Note error={outcome !== 'connected'}>{CONNECT_MESSAGES[outcome]}</Note></View> : null}
        {connect.isError ? <View className="px-4 pb-3"><Note error>The connection could not start. Try again.</Note></View> : null}
      </SettingsCard>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsShell>
  )
}
