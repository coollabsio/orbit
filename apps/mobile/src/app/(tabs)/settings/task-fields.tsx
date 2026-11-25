import { fieldKindLabel } from '@orbit/api/taskFields'
import { useState } from 'react'
import { View } from 'react-native'

import { ActionSheet, ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { Note, SettingsCard, SettingsRow, SettingsShell, SmallOutlineButton, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { Dot } from '../../../components/tasks/pickers'
import { TaskFieldForm } from '../../../components/tasks/TaskFieldForm'
import type { TaskField } from '../../../data/task-api'
import { projectsText } from '../../../data/task-fields'
import { useTaskFieldGallery, useTaskFieldWrites } from '../../../hooks/task-fields'
import { Text } from '../../../ui'

/**
 * The web's Task fields page: the workspace's field gallery. Every member creates fields; the creator, owners and
 * admins (`can_manage`) edit and delete them. A project adds a field in its settings, under Fields.
 */
export default function TaskFieldsScreen() {
  const fields = useTaskFieldGallery()
  const writes = useTaskFieldWrites()
  // `field` null: a new field
  const [form, setForm] = useState<{ field: TaskField | null } | null>(null)
  const [menu, setMenu] = useState<TaskField | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const list = fields.data ?? []

  const askDelete = (field: TaskField) =>
    setConfirm({
      title: `Delete ${field.name}?`,
      description: `${projectsText(field.project_count)}. Every task loses its value of this field. You cannot undo this.`,
      confirmLabel: 'Delete',
      danger: true,
      onConfirm: () => writes.remove.mutate(field),
    })

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={() => fields.refetch()}>
      <SettingsCard
        title="Task fields"
        description="Custom fields that any project of the workspace can add to its tasks, for example a customer, a customer group or a budget. Add a field to a project in its settings, under Fields."
        flush
        actions={<SmallOutlineButton label="New field" icon="plus" onPress={() => setForm({ field: null })} />}
      >
        {fields.isPending ? <SettingsRow first><Note loading>Loading fields…</Note></SettingsRow> : null}
        {fields.isError ? <SettingsRow first><Note error>Fields could not be loaded. Pull to retry.</Note></SettingsRow> : null}
        {fields.isSuccess && list.length === 0 ? <SettingsRow first><Note>No fields yet.</Note></SettingsRow> : null}
        {list.map((field, index) => (
          <SettingsRow key={field.id} first={index === 0} label={`${field.name}, ${fieldKindLabel(field.kind)}. Actions`} onPress={field.can_manage ? () => setMenu(field) : undefined}>
            <View className="min-w-0 flex-1 gap-1">
              <Text numberOfLines={1} className="font-noto-medium text-[13px] text-foreground">
                {field.name}
                <Text className="font-noto text-[13px] text-muted-foreground"> · {fieldKindLabel(field.kind)}</Text>
              </Text>
              {field.options.length > 0 ? (
                <View className="flex-row flex-wrap items-center gap-x-2.5 gap-y-1">
                  {field.options.map((option) => (
                    <View key={option.id} className="flex-row items-center gap-1">
                      <Dot color={option.color} size={6} />
                      <Text className="text-[12px] text-muted-foreground/70">{option.name}</Text>
                    </View>
                  ))}
                </View>
              ) : null}
            </View>
            <Text className="shrink-0 text-[12px] text-muted-foreground">{projectsText(field.project_count)}</Text>
          </SettingsRow>
        ))}
        {writes.remove.isError ? <SettingsRow><Note error>The field could not be deleted.</Note></SettingsRow> : null}
      </SettingsCard>

      <ActionSheet
        visible={menu !== null}
        title={menu?.name}
        actions={
          menu
            ? [
                { label: 'Edit', icon: 'edit', onPress: () => setForm({ field: menu }) },
                { label: 'Delete', icon: 'trash', destructive: true, disabled: writes.remove.isPending, onPress: () => askDelete(menu) },
              ]
            : []
        }
        onClose={() => setMenu(null)}
      />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
      {form ? (
        <TaskFieldForm
          field={form.field}
          submitLabel={form.field ? 'Save' : 'Create field'}
          errorText={form.field ? 'The field could not be saved. Its name may be in use, or someone else changed it; try again.' : 'The field could not be created. Its name may already be in use.'}
          onClose={() => setForm(null)}
          onSave={(body) => {
            writes.remove.reset()
            const { field } = form
            return field ? writes.update.mutateAsync({ field, changes: { name: body.name, options: body.options } }) : writes.create.mutateAsync(body)
          }}
        />
      ) : null}
    </SettingsShell>
  )
}
