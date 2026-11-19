import type { GithubConnection } from '@orbit/api/generated/types.gen'
import { useRouter } from 'expo-router'
import { View } from 'react-native'

import { CONNECT_MESSAGES } from '../../data/github-api'
import { useConnectGithub } from '../../hooks/github'
import { useOrbit } from '../../orbit-context'
import { Button, Text } from '../../ui'

/** The web's `ConnectGithubPrompt`: shown in place of PR content when the member cannot use GitHub yet. */
export function ConnectGithubPrompt({ status }: { status: GithubConnection['status'] }) {
  const router = useRouter()
  const connect = useConnectGithub()
  const canManage = useOrbit().workspace.permissions.includes('integrations.manage')
  if (status === 'connected') return null

  const askAdmin = (problem: string) => (
    <View className="items-center gap-3 py-10">
      <Text className="max-w-sm text-center text-[14px] leading-[20px] text-muted-foreground">{problem}{canManage ? '' : ' Ask a workspace owner or admin.'}</Text>
      {canManage ? <Button label="Open GitHub settings" variant="outline" onPress={() => router.push('/settings/github')} /> : null}
    </View>
  )
  if (status === 'needs_recreate') return askAdmin('To review pull requests in Orbit, re-create the GitHub App of this workspace.')
  if (status === 'unavailable') return askAdmin('This workspace has no GitHub App yet.')
  const broken = status === 'broken'
  const outcome = connect.data
  return (
    <View className="items-center gap-3 py-10">
      <Text className="max-w-sm text-center text-[14px] leading-[20px] text-muted-foreground">
        {broken ? 'GitHub no longer accepts your connection. Connect again to see this pull request.' : 'Connect GitHub to see this pull request.'}
      </Text>
      <Button label={broken ? 'Reconnect GitHub' : 'Connect GitHub'} loading={connect.isPending} onPress={() => connect.mutate()} />
      {connect.isError ? <Text className="text-[12px] leading-[16px] text-destructive">The connection could not start. Try again.</Text> : null}
      {outcome && outcome !== 'connected' ? <Text className="text-center text-[12px] leading-[16px] text-destructive">{CONNECT_MESSAGES[outcome]}</Text> : null}
    </View>
  )
}
