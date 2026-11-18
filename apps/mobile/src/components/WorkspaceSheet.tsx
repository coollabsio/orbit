import { useState } from 'react'
import { ScrollView, View } from 'react-native'

import { useOrbit } from '../orbit-context'
import { useTheme } from '../theme'
import { Icon, ListRow } from '../ui'
import { NewWorkspaceForm } from './NewWorkspaceForm'
import { Sheet } from './tasks/Sheet'

/**
 * The web's `WorkspaceSwitcher` panel: the workspaces, then Create workspace, which swaps the list for the form in
 * the same sheet (iOS cannot open a second modal while this one closes). A new workspace becomes the current one.
 */
export function WorkspaceSheet({ visible, onClose }: { visible: boolean; onClose: () => void }) {
  const { colors } = useTheme()
  const { workspace, workspaces, selectWorkspace } = useOrbit()
  const [creating, setCreating] = useState(false)
  // the form starts empty each time the sheet opens
  const [opened, setOpened] = useState(visible)
  if (visible !== opened) {
    setOpened(visible)
    if (visible) setCreating(false)
  }
  return (
    <Sheet visible={visible} title={creating ? 'New workspace' : 'Workspaces'} onClose={onClose}>
      {creating ? (
        <View className="px-4 pt-2 pb-2">
          <NewWorkspaceForm
            onCancel={() => setCreating(false)}
            onCreated={(created) => {
              selectWorkspace(created.id)
              onClose()
            }}
          />
        </View>
      ) : (
        <ScrollView>
          {workspaces.map((item) => (
            <ListRow
              key={item.id}
              title={item.name}
              subtitle={item.role}
              trailing={item.id === workspace.id ? <Icon name="check" size={18} color={colors.primary} /> : undefined}
              onPress={() => {
                onClose()
                selectWorkspace(item.id)
              }}
            />
          ))}
          <ListRow title="Create workspace" leading={<Icon name="plus" size={18} color={colors.mutedForeground} />} onPress={() => setCreating(true)} />
        </ScrollView>
      )}
    </Sheet>
  )
}
