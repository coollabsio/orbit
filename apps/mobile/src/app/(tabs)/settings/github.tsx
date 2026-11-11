import { useMutation } from '@tanstack/react-query'
import type { GithubWorkspaceSettings } from '@orbit/api/generated/types.gen'
import * as WebBrowser from 'expo-web-browser'
import { type ReactNode, useState } from 'react'
import { Platform, View } from 'react-native'

import { SettingsCard, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { githubManifest } from '../../../data/workspace-settings-api'
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
              <Body>This GitHub App is from an older Orbit version. Re-create it to review pull requests in Orbit. You must then install it again and connect each project to its repository again.</Body>
              {settings.can_manage ? (
                <Button label="Re-create GitHub App" variant="outline" disabled={!settings.key_configured || register.isPending} onPress={() => register.mutate()} />
              ) : (
                <Body muted>Ask a workspace owner or admin to re-create it.</Body>
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

/** The web card body's `text-sm`. */
function Body({ children, muted = false }: { children: ReactNode; muted?: boolean }) {
  return <Text className={`text-[14px] leading-[20px] ${muted ? 'text-muted-foreground' : 'text-foreground'}`}>{children}</Text>
}
