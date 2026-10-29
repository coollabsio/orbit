import { mcpClientCommands, mcpUrl } from '@orbit/api/mcp'
import { View } from 'react-native'

import { ACCOUNT_NAV, Note, RowIcon, RowText, SettingsCard, SettingsRow, SettingsShell } from '../../../components/settings/Shell'
import { useDisconnectOAuth, useOAuthConnections } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { errorMessage } from '../../../session-context'
import { Button, Text } from '../../../ui'

/**
 * The web's Agentic page: the MCP server address, the command that adds it to each client, and the connected clients.
 * The commands run on a computer, so a long press selects them; the app has no clipboard module.
 */
export default function AgenticScreen() {
  const { session } = useOrbit()
  const connections = useOAuthConnections()
  const disconnect = useDisconnectOAuth()
  const url = mcpUrl(session.server)
  const list = connections.data ?? []

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV} refreshing={connections.isRefetching} onRefresh={() => void connections.refetch()}>
      <SettingsCard title="MCP server" description="AI clients such as Claude, Codex and Grok can read your tasks through this address. The client signs in with your Orbit account, and you select the workspace and the projects it can read.">
        <Code text={url} />
      </SettingsCard>

      <SettingsCard title="Connect a client" description="Run the command for your client in a terminal. Then sign in to Orbit in the browser, select the projects and click Allow read access.">
        <View className="gap-4">
          {mcpClientCommands(url).map((entry) => (
            <View key={entry.client} className="gap-1.5">
              <Text className="text-[13px] font-medium">{entry.client}</Text>
              <Code text={entry.command} />
              <Text className="text-xs text-muted-foreground">{entry.signIn}</Text>
            </View>
          ))}
          <Text className="text-xs text-muted-foreground">A client that cannot sign in with OAuth can send a read-only API token instead. A workspace admin creates it in the web app, in Settings → API tokens.</Text>
        </View>
      </SettingsCard>

      <SettingsCard title="Connected apps" description="MCP clients with approved read-only task access. Disconnecting an app stops its access and refresh tokens." flush>
        {connections.isPending ? <View className="p-4"><Note>Loading connections…</Note></View> : null}
        {connections.isError ? <View className="p-4"><Note error>Connections could not be loaded.</Note></View> : null}
        {connections.data && list.length === 0 ? <View className="p-4"><Note>No connected apps.</Note></View> : null}
        {list.map((connection, index) => (
          <SettingsRow key={connection.id} first={index === 0}>
            <RowIcon icon="robot" />
            <RowText title={connection.client_name} subtitle={`${connection.workspace_name} · Read tasks · ${connection.project_names.join(', ')}`} />
            <Button label="Disconnect" variant="ghost" size="sm" disabled={disconnect.isPending} onPress={() => disconnect.mutate(connection.id)} />
          </SettingsRow>
        ))}
        {disconnect.isError ? <View className="p-4"><Note error>The app could not be disconnected. {errorMessage(disconnect.error)}</Note></View> : null}
      </SettingsCard>
    </SettingsShell>
  )
}

function Code({ text }: { text: string }) {
  return (
    <View className="rounded-md bg-muted px-3 py-2.5">
      <Text selectable className="font-mono text-xs leading-5">{text}</Text>
    </View>
  )
}
