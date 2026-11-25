import { FIELD_KINDS, hasOptions, newFieldKinds } from '@orbit/api/taskFields'
import { useRef, useState } from 'react'
import { Pressable, View } from 'react-native'

import type { TaskField, TaskFieldBody, TaskFieldKind, TaskFieldOptionInput } from '../../data/task-api'
import { fieldBody, nextOptionColor, OPTION_LIMIT, optionsProblem } from '../../data/task-fields'
import { useTheme } from '../../theme'
import { Button, IconButton, Input, LabelTip, Text } from '../../ui'
import { Dialog } from '../settings/Dialog'
import { FieldLabel, Select } from '../settings/Shell'
import { Swatches } from './pickers'

type Option = TaskFieldOptionInput & { key: string }

/**
 * New field or Edit field of the gallery (the web's `TaskFieldForm`): name, type and, for the select types, the
 * options with their colours. An existing field keeps its type: the stored values would not fit another one.
 * `gallery` limits the types of a new field. `onSave` resolves to close the dialog; a rejection shows `errorText` and
 * keeps it open.
 */
export function TaskFieldForm({ field, gallery, submitLabel, errorText, onSave, onClose }: { field: TaskField | null; gallery: readonly TaskField[]; submitLabel: string; errorText: string; onSave: (body: TaskFieldBody) => Promise<unknown>; onClose: () => void }) {
  const { colors } = useTheme()
  const nextKey = useRef(0)
  const [name, setName] = useState(field?.name ?? '')
  const [kind, setKind] = useState<TaskFieldKind>(field?.kind ?? 'text')
  const [options, setOptions] = useState<Option[]>(() => (field?.options ?? []).map((option) => ({ id: option.id, name: option.name, color: option.color, key: option.id })))
  const [colorOf, setColorOf] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState(false)
  const problem = hasOptions(kind) ? optionsProblem(options) : null
  const valid = Boolean(name.trim()) && !problem

  const change = (key: string, patch: Partial<TaskFieldOptionInput>) => setOptions(options.map((option) => (option.key === key ? { ...option, ...patch } : option)))
  const add = () => setOptions([...options, { name: '', color: nextOptionColor(options), key: `new-${nextKey.current++}` }])
  const submit = () => {
    if (!valid || busy) return
    setBusy(true)
    setFailed(false)
    onSave(fieldBody(name, kind, options.map(({ key: _key, ...option }) => option))).then(onClose, () => {
      setBusy(false)
      setFailed(true)
    })
  }

  return (
    <Dialog
      visible
      title={field ? 'Edit field' : 'New field'}
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={submitLabel} loading={busy} disabled={!valid} onPress={submit} />
        </>
      }
    >
      <Input label="Name" value={name} onChangeText={setName} placeholder="Customer" maxLength={100} autoFocus={!field} returnKeyType="done" />
      <View className="gap-2">
        {field ? <LabelTip label="Type" text="A field keeps its type: the values tasks have would not fit another one." /> : <FieldLabel>Type</FieldLabel>}
        <Select label="Field type" value={kind} disabled={Boolean(field)} options={(field ? FIELD_KINDS : newFieldKinds(gallery)).map((item) => ({ value: item.kind, label: item.label }))} onChange={setKind} />
      </View>
      {hasOptions(kind) ? (
        <View className="gap-2">
          <FieldLabel>Options</FieldLabel>
          {options.map((option, index) => (
            <View key={option.key} className="gap-1">
              <View className="flex-row items-center gap-2">
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel={`Color of ${option.name.trim() || `option ${index + 1}`}`}
                  accessibilityState={{ expanded: colorOf === option.key }}
                  hitSlop={6}
                  onPress={() => setColorOf(colorOf === option.key ? null : option.key)}
                  className="size-6 items-center justify-center"
                >
                  <View className="size-4 rounded-full" style={{ backgroundColor: option.color }} />
                </Pressable>
                <View className="min-w-0 flex-1">
                  <Input accessibilityLabel={`Option ${index + 1}`} value={option.name} onChangeText={(text) => change(option.key, { name: text })} placeholder="Option name" maxLength={100} returnKeyType="done" />
                </View>
                <IconButton icon="close" label={`Remove ${option.name.trim() || `option ${index + 1}`}`} color={colors.mutedForeground} onPress={() => setOptions(options.filter((item) => item.key !== option.key))} />
              </View>
              {colorOf === option.key ? <Swatches label={`Color of ${option.name.trim() || `option ${index + 1}`}`} className="pl-8 py-1" value={option.color} onChange={(color) => change(option.key, { color })} /> : null}
            </View>
          ))}
          <View className="items-start">
            <Button label="Add option" icon="plus" variant="ghost" disabled={options.length >= OPTION_LIMIT} onPress={add} />
          </View>
          {problem ? (
            <Text accessibilityRole="alert" className="text-[12px] leading-[16px] text-destructive">
              {problem}
            </Text>
          ) : null}
        </View>
      ) : null}
      {failed ? (
        <Text accessibilityRole="alert" className="text-[13px] leading-[19px] text-destructive">
          {errorText}
        </Text>
      ) : null}
    </Dialog>
  )
}
