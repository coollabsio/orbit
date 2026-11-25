import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { TaskField, TaskFieldValue } from '@orbit/api/generated/types.gen'
import { parseFieldInput, selectedOptions } from '@orbit/api/taskFields'
import { router } from 'expo-router'
import { useState } from 'react'
import { View } from 'react-native'
import { keys } from '../../data/keys'
import * as taskApi from '../../data/task-api'
import { fieldKeys } from '../../hooks/task-fields'
import { useOrbit } from '../../orbit-context'
import { Button, Input } from '../../ui'
import { Dot } from './pickers'
import { SheetList, SheetNote, SheetRow } from './Sheet'

/** The fields a project's tasks show, in order. */
export function useProjectTaskFields(projectId: string | null) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: fieldKeys.project(workspaceId, projectId ?? ''), queryFn: () => taskApi.projectTaskFields(client, workspaceId, projectId!), enabled: projectId !== null })
}

export function useTaskFieldValues(taskId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: fieldKeys.values(workspaceId, taskId), queryFn: () => taskApi.taskFieldValues(client, workspaceId, taskId) })
}

/** Sets or clears (`null`) one value; the task shows it at once, and a failure puts the old values back. */
export function useSetTaskFieldValue(taskId: string) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const key = fieldKeys.values(workspaceId, taskId)
  return useMutation({
    mutationFn: ({ fieldId, value }: { fieldId: string; value: unknown }) => taskApi.saveTaskFieldValue(client, workspaceId, taskId, fieldId, value),
    onMutate: async ({ fieldId, value }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const before = queryClient.getQueryData<TaskFieldValue[]>(key)
      const others = (before ?? []).filter((item) => item.field_id !== fieldId)
      queryClient.setQueryData<TaskFieldValue[]>(key, value === null ? others : [...others, { field_id: fieldId, value }])
      return { before }
    },
    onError: (_error, _vars, context) => queryClient.setQueryData(key, context?.before),
    onSuccess: (values) => queryClient.setQueryData(key, values),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: keys.taskActivity(workspaceId, taskId) }),
  })
}

/**
 * The sheet of one field: its options (select kinds; a multi-select stays open), or one input for a text, number,
 * date (`YYYY-MM-DD`) or URL value.
 */
export function TaskFieldSheet({ field, value, onSave, onClose }: { field: TaskField; value: unknown; onSave: (value: unknown) => void; onClose: () => void }) {
  if (field.kind === 'select' || field.kind === 'multi_select') {
    const chosen = selectedOptions(field, value).map((option) => option.id)
    const multi = field.kind === 'multi_select'
    return (
      <SheetList>
        {field.options.map((option) => (
          <SheetRow
            key={option.id}
            label={option.name}
            leading={<Dot color={option.color} />}
            checked={chosen.includes(option.id)}
            onPress={() => {
              if (multi) return onSave(chosen.includes(option.id) ? chosen.filter((id) => id !== option.id) : [...chosen, option.id])
              onClose()
              onSave(chosen[0] === option.id ? null : option.id)
            }}
          />
        ))}
        {chosen.length > 0 ? <SheetRow label="Clear" icon="close" onPress={() => { onClose(); onSave(null) }} /> : null}
        {field.options.length === 0 ? <SheetNote>This field has no options yet. Add them in Settings → Task fields.</SheetNote> : null}
        {field.options.length === 0 ? <SheetRow label="Open Task fields" icon="settings" onPress={() => { onClose(); router.push('/settings/task-fields') }} /> : null}
      </SheetList>
    )
  }
  return <FieldInput field={field} value={value} onSave={(next) => { onClose(); onSave(next) }} />
}

function FieldInput({ field, value, onSave }: { field: TaskField; value: unknown; onSave: (value: unknown) => void }) {
  const [draft, setDraft] = useState(value === null || value === undefined ? '' : String(value))
  const [invalid, setInvalid] = useState(false)
  const date = field.kind === 'date'
  const save = () => {
    const next = date ? dateInput(draft) : parseFieldInput(field.kind, draft)
    if (next === undefined) return setInvalid(true)
    onSave(next)
  }
  return (
    <View className="px-4 pb-2 gap-3">
      <Input
        label={field.name}
        value={draft}
        onChangeText={(text) => { setDraft(text); setInvalid(false) }}
        placeholder={date ? 'YYYY-MM-DD' : field.kind === 'number' ? '0' : field.kind === 'url' ? 'https://...' : field.name}
        keyboardType={field.kind === 'number' ? 'decimal-pad' : field.kind === 'url' ? 'url' : date ? 'numbers-and-punctuation' : 'default'}
        autoCapitalize={field.kind === 'text' ? 'sentences' : 'none'}
        autoCorrect={field.kind === 'text'}
        autoFocus
        returnKeyType="done"
        onSubmitEditing={save}
        error={invalid ? (field.kind === 'number' ? 'Enter a number.' : date ? 'Enter a date as YYYY-MM-DD.' : 'Enter an http or https link.') : undefined}
      />
      <View className="flex-row justify-end gap-2">
        {value !== null && value !== undefined ? <Button label="Clear" variant="ghost" onPress={() => onSave(null)} /> : null}
        <Button label="Save" onPress={save} />
      </View>
    </View>
  )
}

/** A `YYYY-MM-DD` day that exists; empty clears. */
export function dateInput(text: string): string | null | undefined {
  const trimmed = text.trim()
  if (!trimmed) return null
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(trimmed)
  if (!match) return undefined
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])]
  const parsed = new Date(Date.UTC(year, month - 1, day))
  return parsed.getUTCFullYear() === year && parsed.getUTCMonth() === month - 1 && parsed.getUTCDate() === day ? trimmed : undefined
}
