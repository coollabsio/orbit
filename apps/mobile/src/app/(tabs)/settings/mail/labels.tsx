import { useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { Pressable, View } from 'react-native'

import { MailManagerPage } from '../../../../components/settings/mail/MailboxManagersOnly'
import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { Note, SettingsCard, SettingsRow } from '../../../../components/settings/Shell'
import { LabelPill } from '../../../../components/tasks/pickers'
import { mailSettingsKeys, type MailLabel } from '../../../../data/mail-mailboxes-api'
import { mailProblem } from '../../../../data/mail-settings'
import { LABEL_PALETTE } from '../../../../data/tasks'
import { useMailLabelList, useMailLabelWrites } from '../../../../hooks/mail-mailboxes'
import { useOrbit } from '../../../../orbit-context'
import { Button, Input } from '../../../../ui'

/** The web's Settings → Mail → Mail labels: create, rename, recolor, delete. Every member can put them on the threads they read. */
export default function MailLabelsScreen() {
  const queryClient = useQueryClient()
  const { workspaceId } = useOrbit()
  return (
    <MailManagerPage title="Mail labels" onRefresh={() => queryClient.refetchQueries({ queryKey: mailSettingsKeys.labels(workspaceId) })}>
      <MailLabels />
    </MailManagerPage>
  )
}

function MailLabels() {
  const labels = useMailLabelList()
  const { create, update, remove } = useMailLabelWrites()
  const [editingId, setEditingId] = useState<string>()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const sorted = [...(labels.data ?? [])].sort((a, b) => a.name.localeCompare(b.name))

  const deleteLabel = (label: MailLabel) =>
    setConfirm({
      title: `Delete ${label.name}?`,
      description: 'The label is removed from every thread that has it. You cannot undo this.',
      confirmLabel: 'Delete',
      danger: true,
      onConfirm: () => remove.mutate(label.id),
    })

  return (
    <SettingsCard title="Mail labels" description="Labels for mail threads, the same for everyone in the workspace." flush>
      <SettingsRow first>
        <LabelForm submitLabel="Create label" pending={create.isPending} onSubmit={(name, color) => create.mutateAsync({ name, color })} />
      </SettingsRow>
      {create.isError ? <SettingsRow><Note error>{mailProblem(create.error, 'The label could not be created.')}</Note></SettingsRow> : null}
      {labels.isPending ? <SettingsRow><Note loading>Loading labels…</Note></SettingsRow> : null}
      {labels.isError ? <SettingsRow><Note error>Labels could not be loaded.</Note></SettingsRow> : null}
      {labels.isSuccess && sorted.length === 0 ? <SettingsRow><Note>No labels yet.</Note></SettingsRow> : null}
      {sorted.map((label) =>
        editingId === label.id ? (
          <SettingsRow key={label.id}>
            <LabelForm
              initial={label}
              submitLabel="Save"
              pending={update.isPending}
              onCancel={() => setEditingId(undefined)}
              onSubmit={async (name, color) => {
                await update.mutateAsync({ labelId: label.id, name, color })
                setEditingId(undefined)
              }}
            />
          </SettingsRow>
        ) : (
          <SettingsRow key={label.id}>
            <View className="min-w-0 flex-1 flex-row">
              <LabelPill label={label} />
            </View>
            <Button
              label="Edit"
              variant="ghost"
              onPress={() => {
                update.reset()
                setEditingId(label.id)
              }}
            />
            <Button label="Delete" variant="ghost" disabled={remove.isPending} onPress={() => deleteLabel(label)} />
          </SettingsRow>
        ),
      )}
      {update.isError ? <SettingsRow><Note error>{mailProblem(update.error, 'The label could not be saved.')}</Note></SettingsRow> : null}
      {remove.isError ? <SettingsRow><Note error>{mailProblem(remove.error, 'The label could not be deleted.')}</Note></SettingsRow> : null}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}

function LabelForm({ initial, submitLabel, pending, onSubmit, onCancel }: { initial?: MailLabel; submitLabel: string; pending: boolean; onSubmit: (name: string, color: string) => Promise<unknown>; onCancel?: () => void }) {
  const [name, setName] = useState(initial?.name ?? '')
  const [color, setColor] = useState(initial?.color ?? LABEL_PALETTE[0]!.color)
  const submit = async () => {
    const trimmed = name.trim()
    if (!trimmed || pending) return
    try {
      await onSubmit(trimmed, color)
      if (!initial) setName('')
    } catch {
      // the page shows the mutation error
    }
  }
  return (
    <View className="min-w-0 flex-1 gap-3">
      <Input
        accessibilityLabel="Label name"
        placeholder="Label name"
        maxLength={80}
        value={name}
        onChangeText={setName}
        autoFocus={Boolean(initial)}
        returnKeyType="done"
        onSubmitEditing={() => void submit()}
      />
      <Swatches value={color} onChange={setColor} />
      <View className="flex-row flex-wrap gap-2">
        <Button label={submitLabel} disabled={!name.trim() || pending} onPress={() => void submit()} />
        {onCancel ? <Button label="Cancel" variant="ghost" onPress={onCancel} /> : null}
      </View>
    </View>
  )
}

/** The web's `LabelColorPicker`: the label palette as round swatches; the chosen one wears a ring in its own colour. */
function Swatches({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  return (
    <View accessibilityRole="radiogroup" accessibilityLabel="Label color" className="flex-row flex-wrap gap-2.5">
      {LABEL_PALETTE.map((swatch) => {
        const checked = swatch.color === value
        return (
          <Pressable
            key={swatch.color}
            accessibilityRole="radio"
            accessibilityLabel={swatch.name}
            accessibilityState={{ checked }}
            hitSlop={4}
            onPress={() => onChange(swatch.color)}
            className="size-6.5 items-center justify-center rounded-full border-[1.5px]"
            style={{ borderColor: checked ? swatch.color : 'transparent' }}
          >
            <View className="size-4.5 rounded-full" style={{ backgroundColor: swatch.color }} />
          </Pressable>
        )
      })}
    </View>
  )
}
