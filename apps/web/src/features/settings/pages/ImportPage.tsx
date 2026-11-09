import { Link } from 'react-router'
import { toast } from 'sonner'
import { ApiProblem } from '@orbit/api/problem'
import type { ImportPerson, ImportSource } from '@orbit/api/generated/types.gen'
import { Import, People as Users } from 'reicon-react'
import { SettingsCard } from '@/components/common/SettingsCard'
import { buttonVariants } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { useImportPeople, useMapImportPerson } from '@/features/settings/api/importPeople'
import { useMembers } from '@/features/workspaces/api'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

const SOURCES = [
  { name: 'Notion', description: 'Import pages and databases into Docs.', to: '/docs/import' },
  { name: 'Obsidian', description: 'Import a vault (.zip) into Docs.', to: '/docs/import/obsidian' },
  { name: 'Asana', description: 'Import projects and their tasks into Tasks.', to: '/tasks-import' },
  { name: 'Linear', description: 'Import teams and their issues into Tasks.', to: '/tasks-import/linear' },
  { name: 'Trello', description: 'Import boards and their cards into Tasks.', to: '/tasks-import/trello' },
]

const SOURCE_NAMES: Record<ImportSource, string> = { asana: 'Asana', linear: 'Linear', trello: 'Trello' }
/** The Select value of "Not assigned" (Select values are strings). */
const UNMAPPED = 'none'

function tasks(count: number) {
  return count === 1 ? '1 task' : `${count} tasks`
}

/** Where to bring data into the workspace from other tools. */
export function ImportPage() {
  const canManage = useCan('members.manage')
  return (
    <>
      <SettingsCard title="Import" flush>
        {SOURCES.map((source) => (
          <SettingsRow key={source.name} className="flex-nowrap">
            <RowIcon>
              <Import />
            </RowIcon>
            <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
              <span className="font-medium text-foreground">{source.name}</span>
              <span className="text-xs text-muted-foreground">{source.description}</span>
            </div>
            <Link to={source.to} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
              Import from {source.name}
            </Link>
          </SettingsRow>
        ))}
      </SettingsCard>
      {canManage ? <ImportPeople /> : null}
    </>
  )
}

/** Assignees of imported tasks that no member had the email of: an admin picks the member who gets their tasks. */
function ImportPeople() {
  const { workspace } = useWorkspace()
  const people = useImportPeople(workspace.id)
  const members = useMembers(workspace.id)
  const mapPerson = useMapImportPerson(workspace.id)
  const items = people.data ?? []
  if (items.length === 0) return null
  const active = (members.data ?? []).filter((member) => !member.suspended).sort((a, b) => a.name.localeCompare(b.name))

  const change = (person: ImportPerson, value: string | null) => {
    const userId = value === null || value === UNMAPPED ? null : value
    if (userId === (person.user_id ?? null)) return
    mapPerson.mutate(
      { personId: person.id, userId },
      {
        onError: (error) => toast.error(error instanceof ApiProblem ? error.detail : `${person.name}'s tasks could not be assigned.`),
      },
    )
  }

  return (
    <SettingsCard
      title="People from imports"
      description="Assignees of imported tasks that no member had the email of. Pick a member to assign all of their tasks; the email does not need to match."
      flush
    >
      {items.map((person) => {
        const value = person.user_id ?? UNMAPPED
        const options = [
          { value: UNMAPPED, label: 'Not assigned' },
          ...active.map((member) => ({ value: member.id, label: member.name })),
          ...(person.user_id && !active.some((member) => member.id === person.user_id) ? [{ value: person.user_id, label: 'Former member' }] : []),
        ]
        const sending = mapPerson.isPending && mapPerson.variables?.personId === person.id
        // the pick shows at once; a job then moves the tasks, which takes a while for thousands of them
        const shown = sending ? (mapPerson.variables?.userId ?? UNMAPPED) : value
        const saving = sending || person.assigning
        const toMember = sending ? Boolean(mapPerson.variables?.userId) : Boolean(person.user_id)
        return (
          <SettingsRow key={person.id} className="flex-nowrap max-[899px]:flex-wrap">
            <RowIcon>
              <Users />
            </RowIcon>
            <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
              <span className="truncate font-medium text-foreground">{person.name}</span>
              {saving ? (
                <span role="status" className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <Spinner className="size-3" />
                  {toMember ? 'Assigning' : 'Unassigning'} {tasks(person.task_count)}…
                </span>
              ) : (
                <span className="truncate text-xs text-muted-foreground">
                  {[SOURCE_NAMES[person.source], person.email, tasks(person.task_count)].filter(Boolean).join(' · ')}
                </span>
              )}
            </div>
            <Select items={options} value={shown} disabled={saving} onValueChange={(next) => change(person, next)}>
              <SelectTrigger aria-label={`Member for ${person.name}`} size="sm" className="w-48 max-[899px]:w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </SettingsRow>
        )
      })}
    </SettingsCard>
  )
}
