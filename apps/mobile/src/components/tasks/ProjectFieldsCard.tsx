import { fieldKindLabel } from '@orbit/api/taskFields'
import { router } from 'expo-router'
import { useState } from 'react'

import { canInProject } from '../../data/permissions'
import type { TaskField } from '../../data/task-api'
import { moveId } from '../../data/task-fields'
import { useSetProjectTaskFields, useTaskFieldGallery, useTaskFieldWrites } from '../../hooks/task-fields'
import { useOrbit } from '../../orbit-context'
import { useTheme } from '../../theme'
import { Icon, Text } from '../../ui'
import { ActionSheet } from '../settings/Dialog'
import { Note, RowText, SettingsCard, SettingsRow, SmallOutlineButton } from '../settings/Shell'
import { Sheet, SheetList, SheetRow } from './Sheet'
import { useProjectTaskFields } from './TaskFields'
import { TaskFieldForm } from './TaskFieldForm'

/**
 * The gallery fields this project's tasks show, in order (the web's `ProjectFieldsCard`): add one (or a new one),
 * move, remove. Without `projects.manage` the list is read-only.
 */
export function ProjectFieldsCard({ projectId }: { projectId: string }) {
  const { workspace } = useOrbit()
  const { colors } = useTheme()
  const canManage = canInProject(workspace, 'projects.manage', projectId)
  const shown = useProjectTaskFields(projectId)
  const gallery = useTaskFieldGallery()
  const setFields = useSetProjectTaskFields(projectId)
  const { create } = useTaskFieldWrites()
  const [adding, setAdding] = useState(false)
  const [creating, setCreating] = useState(false)
  const [menu, setMenu] = useState<TaskField | null>(null)
  const list = shown.data ?? []
  const ids = list.map((field) => field.id)
  const addable = (gallery.data ?? []).filter((field) => !ids.includes(field.id))
  const index = menu ? ids.indexOf(menu.id) : -1

  const save = (next: string[]) => setFields.mutate(next)
  const openGallery = () => router.push('/settings/task-fields')

  return (
    <SettingsCard
      title="Fields"
      description="Custom fields that this project's tasks show, in this order. The fields come from the workspace's field gallery, so other projects can use the same field. A task keeps its values when a field is removed here, and shows them again when the field comes back."
      flush
      actions={canManage ? <SmallOutlineButton label="Add field" icon="plus" onPress={() => setAdding(true)} /> : undefined}
    >
      {shown.isPending ? <SettingsRow first><Note loading>Loading fields…</Note></SettingsRow> : null}
      {shown.isError ? <SettingsRow first><Note error>Fields could not be loaded. Pull to retry.</Note></SettingsRow> : null}
      {shown.isSuccess && list.length === 0 ? (
        <SettingsRow first>
          <Note>
            No fields yet. Add one from the workspace's{' '}
            <Text accessibilityRole="link" onPress={openGallery} className="text-[13px] leading-[19px] text-foreground underline">
              field gallery
            </Text>
            .
          </Note>
        </SettingsRow>
      ) : null}
      {list.map((field, at) => (
        <SettingsRow key={field.id} first={at === 0} label={`${field.name}, ${fieldKindLabel(field.kind)}. Actions`} onPress={canManage ? () => setMenu(field) : undefined}>
          <RowText title={field.name} subtitle={fieldKindLabel(field.kind)} />
          {canManage ? <Icon name="more" size={16} color={colors.mutedForeground} /> : null}
        </SettingsRow>
      ))}
      {setFields.isError ? <SettingsRow><Note error>The project's fields could not be saved.</Note></SettingsRow> : null}

      <ActionSheet
        visible={menu !== null}
        title={menu?.name}
        actions={[
          { label: 'Move up', icon: 'arrowUp', disabled: index <= 0 || setFields.isPending, onPress: () => save(moveId(ids, index, -1)) },
          { label: 'Move down', icon: 'arrowDown', disabled: index < 0 || index >= ids.length - 1 || setFields.isPending, onPress: () => save(moveId(ids, index, 1)) },
          { label: 'Remove from project', icon: 'close', destructive: true, disabled: setFields.isPending, onPress: () => menu && save(ids.filter((id) => id !== menu.id)) },
        ]}
        onClose={() => setMenu(null)}
      />
      <Sheet visible={adding} title="Add field" onClose={() => setAdding(false)}>
        <SheetList>
          {addable.map((field) => (
            <SheetRow
              key={field.id}
              label={field.name}
              subtitle={fieldKindLabel(field.kind)}
              onPress={() => {
                setAdding(false)
                save([...ids, field.id])
              }}
            />
          ))}
          <SheetRow
            label="New field…"
            icon="plus"
            onPress={() => {
              setAdding(false)
              setCreating(true)
            }}
          />
          <SheetRow
            label="Manage field gallery"
            icon="settings"
            onPress={() => {
              setAdding(false)
              openGallery()
            }}
          />
        </SheetList>
      </Sheet>
      {creating ? (
        <TaskFieldForm
          field={null}
          submitLabel="Create and add"
          errorText="The field could not be created. Its name may already be in use."
          onClose={() => setCreating(false)}
          onSave={async (body) => {
            const field = await create.mutateAsync(body)
            await setFields.mutateAsync([...ids, field.id])
          }}
        />
      ) : null}
    </SettingsCard>
  )
}
