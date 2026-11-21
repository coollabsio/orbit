import { useMutation } from '@tanstack/react-query'
import type { GithubWorkspaceSettings } from '@orbit/api/generated/types.gen'
import * as WebBrowser from 'expo-web-browser'
import { type ReactNode, useState } from 'react'
import { Platform, View } from 'react-native'

import { SettingsCard, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { githubAppClient, githubManifest } from '../../../data/workspace-settings-api'
import { useGithubSettings } from '../../../hooks/workspace-settings'
import { useOrbit } from '../../../orbit-context'
import { errorMessage } from '../../../session-context'
import { Button, Input, Text } from '../../../ui'

/** The workspace's GitHub App: register it, then install it on repositories (both on GitHub, in the browser). */
export default function GithubScreen() {
  const query = useGithubSettings()
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={() => query.refetch()}>
      {query.isError ? (
        <SettingsCard title="GitHub">
          <Body>GitHub settings are not available.</Body>
        </SettingsCard>
      ) : query.data === undefined ? (
        <SettingsCard title="GitHub">
          <Body>Loading GitHub settings.</Body>
        </SettingsCard>
      ) : (
        <Github settings={query.data} refetch={query.refetch} />
      )}
    </SettingsShell>
  )
}

function Github({ settings, refetch }: { settings: GithubWorkspaceSettings; refetch: () => Promise<unknown> }) {
  const { client, session, workspaceId } = useOrbit()
  const [organization, setOrganization] = useState('')
  // GitHub's manifest flow takes a form POST. The app cannot post one into the system browser, so it opens the web
  // app's GitHub page there; the Expo web build posts the form itself, as the web does.
  const register = useMutation({
    mutationFn: async () => {
      if (Platform.OS !== 'web') {
        await WebBrowser.openBrowserAsync(`${session.server}/settings/github`)
        await refetch()
        return
      }
      const data = await githubManifest(client, workspaceId, organization.trim())
      const form = document.createElement('form')
      form.method = 'POST'
      form.action = data.action
      const input = document.createElement('input')
      input.type = 'hidden'
      input.name = 'manifest'
      input.value = JSON.stringify(data.manifest)
      form.append(input)
      document.body.append(form)
      form.submit()
    },
  })
  const install = async (url: string) => {
    await WebBrowser.openBrowserAsync(url)
    await refetch()
  }

  return (
    <SettingsCard title="GitHub" description={`Manage the GitHub App for this workspace. Connect repositories to projects in each project's settings. ${settings.app_slug ? "After installation, open a project's settings to select a repository and issue label." : 'Register a GitHub App from Orbit. GitHub will return you to this workspace.'}`}>
      {!settings.app_slug ? (
        <View className="items-start gap-2">
          {settings.can_manage ? (
            <>
              {Platform.OS === 'web' ? (
                // the web's `Label` (leading-none) above the field, 6px apart
                <View className="self-stretch gap-1.5">
                  <Text className="font-noto-medium text-[14px] leading-[14px]">GitHub organization (optional)</Text>
                  <Input accessibilityLabel="GitHub organization (optional)" value={organization} onChangeText={setOrganization} placeholder="Leave blank for a personal App" autoCapitalize="none" autoCorrect={false} />
                </View>
              ) : null}
              {!settings.key_configured ? <Text className="text-[12px] leading-[16px] text-destructive">Orbit is not ready to register a GitHub App yet. Ask the Orbit administrator to set the app key.</Text> : null}
              <Button label="Register GitHub App" disabled={!settings.key_configured || register.isPending} onPress={() => register.mutate()} />
              {register.isError ? <Text className="text-[12px] leading-[16px] text-destructive">{errorMessage(register.error) || 'App registration could not start.'}</Text> : null}
            </>
          ) : (
            <Body muted>Ask a workspace owner or admin to register the App.</Body>
          )}
        </View>
      ) : (
        <View className="items-start gap-3">
          {settings.needs_recreate ? (
            <View className="self-stretch items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2.5">
              <Body>This GitHub App is from an older Orbit version. Upgrade it to review pull requests in Orbit.</Body>
              {settings.can_manage ? (
                <>
                  <UpgradeGithubApp keyConfigured={settings.key_configured} refetch={refetch} />
                  <Text className="text-[12px] leading-[16px] text-muted-foreground">You can also re-create the App. You must then install it again and connect each project to its repository again.</Text>
                  <Button label="Re-create GitHub App" variant="outline" disabled={!settings.key_configured || register.isPending} onPress={() => register.mutate()} />
                </>
              ) : (
                <Body muted>Ask a workspace owner or admin to upgrade it.</Body>
              )}
            </View>
          ) : null}
          <Body>App: {settings.app_slug}</Body>
          {settings.can_manage && settings.install_url ? <Button label="Install or change repositories" variant="outline" onPress={() => void install(settings.install_url!)} /> : null}
          <View className="self-stretch">
            <Text className="mb-2 font-noto-medium text-[14px] leading-[20px]">Installed repositories</Text>
            {settings.repositories.length > 0 ? (
              <View accessibilityLabel="Installed repositories" className="gap-2">
                {settings.repositories.map((item) => (
                  <View key={`${item.installation_id}:${item.repository}`} className="rounded-md border border-border px-3 py-2">
                    <Body>{item.repository}</Body>
                  </View>
                ))}
              </View>
            ) : (
              <Body muted>No repositories installed yet.</Body>
            )}
          </View>
        </View>
      )}
    </SettingsCard>
  )
}

const PERMISSIONS = 'Contents: read and write, Pull requests: read and write, Issues: read, Checks: read, Commit statuses: read, Metadata: read'
const EVENTS = 'Issues, Pull request, Pull request review, Pull request review comment, Pull request review thread, Check suite, Check run, Status'

/** Adds the OAuth client to an app from before PR review, so its installations and project connections stay. */
function UpgradeGithubApp({ keyConfigured, refetch }: { keyConfigured: boolean; refetch: () => Promise<unknown> }) {
  const { client, session, workspaceId } = useOrbit()
  const [clientId, setClientId] = useState('')
  const [clientSecret, setClientSecret] = useState('')
  const save = useMutation({
    mutationFn: () => githubAppClient(client, workspaceId, { client_id: clientId.trim(), client_secret: clientSecret.trim() }),
    onSuccess: () => refetch(),
  })
  const steps = [
    "On GitHub, open the App's settings.",
    `Add this callback URL: ${session.server.replace(/\/+$/, '')}/api/v1/integrations/github/user/callback`,
    `Under Permissions, set ${PERMISSIONS}.`,
    `Subscribe to these events: ${EVENTS}.`,
    'Generate a new client secret. Paste it and the Client ID here.',
    'On each installation, accept the new permissions.',
  ]
  return (
    <View className="self-stretch gap-2">
      <View className="gap-1">
        {steps.map((step, index) => (
          <Body key={step}>{`${index + 1}. ${step}`}</Body>
        ))}
      </View>
      <Input label="Client ID" value={clientId} onChangeText={setClientId} autoCapitalize="none" autoCorrect={false} />
      <Input label="Client secret" value={clientSecret} onChangeText={setClientSecret} secureTextEntry autoCapitalize="none" autoCorrect={false} />
      <View className="items-start">
        <Button label="Upgrade GitHub App" disabled={!keyConfigured || save.isPending || !clientId.trim() || !clientSecret.trim()} onPress={() => save.mutate()} />
      </View>
      {save.isError ? <Text className="text-[12px] leading-[16px] text-destructive">{errorMessage(save.error) || 'The App could not be upgraded.'}</Text> : null}
    </View>
  )
}

/** The web card body's `text-sm`. */
function Body({ children, muted = false }: { children: ReactNode; muted?: boolean }) {
  return <Text className={`text-[14px] leading-[20px] ${muted ? 'text-muted-foreground' : 'text-foreground'}`}>{children}</Text>
}
