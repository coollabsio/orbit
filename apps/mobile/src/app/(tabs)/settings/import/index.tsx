import { type Href, router } from 'expo-router'
import { View } from 'react-native'

import { SettingsCard, SettingsShell, WORKSPACE_NAV } from '../../../../components/settings/Shell'
import { useTheme } from '../../../../theme'
import { Button, Icon, Text } from '../../../../ui'

const SOURCES: readonly { name: string; description: string; href: Href }[] = [
  { name: 'Notion', description: 'Import pages and databases into Docs.', href: '/settings/import/notion' },
  { name: 'Obsidian', description: 'Import a vault (.zip) into Docs.', href: '/settings/import/obsidian' },
  { name: 'Asana', description: 'Import projects and their tasks into Tasks.', href: '/settings/import/asana' },
  { name: 'Linear', description: 'Import teams and their issues into Tasks.', href: '/settings/import/linear' },
  { name: 'Trello', description: 'Import boards and their cards into Tasks.', href: '/settings/import/trello' },
]

/** The web's Settings → Import: where to bring data into the workspace from other tools. */
export default function ImportScreen() {
  const { colors } = useTheme()
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV}>
      <SettingsCard title="Import" flush>
        {SOURCES.map((source) => (
          // the web's `SettingsRow` here has no hairline between the sources
          <View key={source.name} className="flex-row items-center gap-3 px-4 py-3">
            <View className="size-9 items-center justify-center rounded-lg bg-muted">
              <Icon name="import" size={18} color={colors.mutedForeground} />
            </View>
            <View className="min-w-0 flex-1 gap-0.5">
              <Text className="font-noto-medium text-[13px]">{source.name}</Text>
              <Text className="text-[12px] leading-[16px] text-muted-foreground">{source.description}</Text>
            </View>
            <Button label={`Import from ${source.name}`} variant="outline" size="sm" onPress={() => router.push(source.href)} />
          </View>
        ))}
      </SettingsCard>
    </SettingsShell>
  )
}
