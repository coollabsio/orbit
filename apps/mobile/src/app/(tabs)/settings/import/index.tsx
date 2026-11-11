import { type Href, router } from 'expo-router'
import { useState } from 'react'
import { View } from 'react-native'

import { Note, RowIcon, RowText, Select, SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../../components/settings/Shell'
import { IMPORT_SOURCE_NAMES, type ImportPerson } from '../../../../data/import-api'
import { useImportPeople, useMapImportPerson } from '../../../../hooks/imports'
import { useOrbit } from '../../../../orbit-context'
import { useMembers } from '../../../../queries'
import { errorMessage } from '../../../../session-context'
import { useTheme } from '../../../../theme'
import { Button, Icon, Spinner, LabelTip } from '../../../../ui'

const SOURCES: readonly { name: string; description: string; href: Href }[] = [
  { name: 'Notion', description: 'Import pages and databases into Docs.', href: '/settings/import/notion' },
  { name: 'Obsidian', description: 'Import a vault (.zip) into Docs.', href: '/settings/import/obsidian' },
  { name: 'Asana', description: 'Import projects and their tasks into Tasks.', href: '/settings/import/asana' },
  { name: 'Linear', description: 'Import teams and their issues into Tasks.', href: '/settings/import/linear' },
  { name: 'Trello', description: 'Import boards and their cards into Tasks.', href: '/settings/import/trello' },
  { name: 'Orbit', description: 'Import projects, pages and chats from another Orbit instance.', href: '/settings/import/orbit' },
]

/** The Select value of "Not assigned". */
const UNMAPPED = 'none'

function tasks(count: number) {
  return count === 1 ? '1 task' : `${count} tasks`
}

/** The web's Settings → Import: where to bring data into the workspace from other tools. */
export default function ImportScreen() {
  const { colors } = useTheme()
  const { workspace } = useOrbit()
  const canManage = workspace.permissions.includes('members.manage')
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV}>
      <SettingsCard title="Import" flush>
        {SOURCES.map((source) => (
          // the web's `SettingsRow` here has no hairline between the sources
          <View key={source.name} className="flex-row items-center gap-3 px-4 py-3">
            <View className="size-9 items-center justify-center rounded-lg bg-muted">
              <Icon name="import" size={18} color={colors.mutedForeground} />
            </View>
            <View className="min-w-0 flex-1">
              <LabelTip label={source.name} text={source.description} className="font-noto-medium text-[13px]" />
            </View>
            <Button label={`Import from ${source.name}`} variant="outline" size="sm" onPress={() => router.push(source.href)} />
          </View>
        ))}
      </SettingsCard>
      {canManage ? <ImportPeople /> : null}
    </SettingsShell>
  )
}

/** The web's "People from imports": assignees no member had the email of, each given to a member the admin picks. */
function ImportPeople() {
  const people = useImportPeople(true)
  const members = useMembers()
  const mapPerson = useMapImportPerson()
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null)
  const items = people.data ?? []
  if (items.length === 0) return null
  const active = (members.data ?? []).filter((member) => !member.suspended_at).sort((a, b) => a.display_name.localeCompare(b.display_name))

  const change = (person: ImportPerson, value: string) => {
    const userId = value === UNMAPPED ? null : value
    if (userId === (person.user_id ?? null)) return
    setNote(null)
    mapPerson.mutate(
      { personId: person.id, userId },
      {
        onError: (error) => setNote({ text: errorMessage(error), error: true }),
      },
    )
  }

  return (
    <SettingsCard
      title="People from imports"
      description="Assignees of imported tasks that no member had the email of. Pick a member to assign all of their tasks; the email does not need to match."
      flush
    >
      {items.map((person, index) => {
        const options = [
          { value: UNMAPPED, label: 'Not assigned' },
          ...active.map((member) => ({ value: member.user_id, label: member.display_name })),
          ...(person.user_id && !active.some((member) => member.user_id === person.user_id) ? [{ value: person.user_id, label: 'Former member' }] : []),
        ]
        const sending = mapPerson.isPending && mapPerson.variables?.personId === person.id
        const saving = sending || person.assigning
        const toMember = sending ? Boolean(mapPerson.variables?.userId) : Boolean(person.user_id)
        const subtitle = saving
          ? `${toMember ? 'Assigning' : 'Unassigning'} ${tasks(person.task_count)}…`
          : [IMPORT_SOURCE_NAMES[person.source], person.email, tasks(person.task_count)].filter(Boolean).join(' · ')
        return (
          <SettingsRow key={person.id} first={index === 0}>
            <RowIcon icon="people" />
            <RowText title={person.name} subtitle={subtitle} />
            {saving ? <Spinner /> : null}
            <View className="w-40" pointerEvents={mapPerson.isPending || saving ? 'none' : 'auto'} style={{ opacity: saving ? 0.5 : 1 }}>
              <Select
                label={`Member for ${person.name}`}
                size="sm"
                value={sending ? (mapPerson.variables?.userId ?? UNMAPPED) : (person.user_id ?? UNMAPPED)}
                options={options}
                onChange={(value) => change(person, value)}
              />
            </View>
          </SettingsRow>
        )
      })}
      {note ? (
        <View className="border-t border-border p-4">
          <Note error={note.error}>{note.text}</Note>
        </View>
      ) : null}
    </SettingsCard>
  )
}
