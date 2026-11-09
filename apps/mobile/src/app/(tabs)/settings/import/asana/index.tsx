import type { AsanaImport, AsanaImportProject, AsanaImportTree } from '@orbit/api/generated/types.gen'
import { router, useLocalSearchParams } from 'expo-router'
import { useMemo, useState } from 'react'
import { ScrollView, View } from 'react-native'

import { type FlowSource, ImportFlow, NoteLine, ResultStep } from '../../../../../components/import/Flow'
import {
  Checkbox,
  ChoiceRow,
  ChooserFooter,
  ChooserToolbar,
  ConnectError,
  ConnectSection,
  ExternalLink,
  ImportBody,
  ImportedProjects,
  ImportScreen,
  Paragraph,
  RecentImports,
  Strong,
  WarningLine,
  WarningStrip,
} from '../../../../../components/import/Parts'
import { Badge } from '../../../../../components/settings/Shell'
import { createAsana, importErrorMessage, importNotes, isImportInProgress } from '../../../../../data/import-api'
import { useConnectImport, useStartImport } from '../../../../../hooks/imports'
import { useOrbit } from '../../../../../orbit-context'
import { useProjects } from '../../../../../queries'
import { Button, Input, Text } from '../../../../../ui'

const TOKEN_URL = 'https://app.asana.com/0/my-apps'
const HOME = '/settings/import/asana'

const SOURCE: FlowSource<'asana'> = {
  kind: 'asana',
  title: 'Import from Asana',
  chooseTitle: 'Choose projects',
  home: HOME,
  detail: (item) => item.asana_user_name,
  scanning: () => ({ headline: 'Finding your Asana projects…', text: 'Listing every workspace and project the token can see.' }),
  cancel: {
    before: 'Orbit deletes the Asana token. You can start a new import at any time.',
    started: 'Projects and tasks already imported stay. The import stops before the next task.',
  },
  importing: {
    headline: (item) => (item.status === 'queued' ? 'Waiting for the import to start…' : item.progress.total === 0 ? 'Reading projects…' : 'Importing tasks…'),
    unit: 'tasks',
    note: 'You can leave this page; the import continues on the server. Asana allows about two requests per second, so large projects take a while.',
  },
  treeMissing: 'The project list is not available.',
}

/** The web's Asana import (`/tasks-import/:importId?`): paste a token, choose projects, follow the import. */
export default function AsanaImportScreen() {
  const { importId } = useLocalSearchParams<{ importId?: string }>()
  return <ImportFlow source={SOURCE} importId={importId} connect={<ConnectStep />} chooser={(item, tree) => <Chooser item={item} tree={tree} />} result={(item) => <Result item={item} />} />
}

function ConnectStep() {
  const { client, workspaceId } = useOrbit()
  const connect = useConnectImport('asana', (created) => router.replace(`${HOME}/${created.id}`))
  const [token, setToken] = useState('')
  const value = token.trim()
  const submit = () => {
    if (!value) return
    void connect.submit(async () => {
      const created = await createAsana(client, workspaceId, value)
      setToken('')
      return created
    })
  }
  return (
    <ImportScreen title={SOURCE.title}>
      <ImportBody>
        <ConnectSection title="Connect Asana"
          intro={
            <Paragraph>
              In Asana, open <ExternalLink url={TOKEN_URL} />, choose <Strong>Create new token</Strong>, name it (for example “Orbit import”) and copy it. The token can read everything you can see in Asana. Orbit keeps it encrypted only until the import finishes; you can delete it in Asana afterwards.
            </Paragraph>
          }
        >
          <View className="gap-3">
            <Input label="Asana personal access token" secureTextEntry autoComplete="off" autoCapitalize="none" autoCorrect={false} placeholder="2/1200…" value={token} onChangeText={setToken} onSubmitEditing={submit} />
            {connect.error ? <ConnectError message={importErrorMessage('asana', connect.error)} running={isImportInProgress(connect.error) && connect.running ? `${HOME}/${connect.running.id}` : null} /> : null}
            <View className="flex-row">
              <Button label="Find projects" loading={connect.pending} disabled={!value} onPress={submit} />
            </View>
          </View>
        </ConnectSection>
        <RecentImports imports={connect.imports} name={(item) => item.asana_user_name || 'Asana'} unit="tasks" href={(item) => `${HOME}/${item.id}`} />
      </ImportBody>
    </ImportScreen>
  )
}

/** One workspace's projects grouped by team, teams in the order the scan sorted them. */
function groupByTeam(projects: AsanaImportProject[]): { team: string; projects: AsanaImportProject[] }[] {
  const groups = new Map<string, AsanaImportProject[]>()
  for (const project of projects) {
    const team = project.team_name ?? 'No team'
    groups.set(team, [...(groups.get(team) ?? []), project])
  }
  return [...groups].map(([team, list]) => ({ team, projects: list }))
}

function checkState(ids: string[], selected: ReadonlySet<string>) {
  const count = ids.filter((id) => selected.has(id)).length
  return { checked: count > 0 && count === ids.length, indeterminate: count > 0 && count < ids.length }
}

/** Pick projects. Active projects start selected, archived ones do not. */
function Chooser({ item, tree }: { item: AsanaImport; tree: AsanaImportTree }) {
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set(tree.workspaces.flatMap((workspace) => workspace.projects.filter((project) => !project.archived).map((project) => project.gid))))
  const [query, setQuery] = useState('')
  const start = useStartImport('asana')
  const needle = query.trim().toLowerCase()
  const workspaces = useMemo(
    () =>
      tree.workspaces
        .map((workspace) => ({ ...workspace, projects: needle ? workspace.projects.filter((project) => `${project.name} ${project.team_name ?? ''}`.toLowerCase().includes(needle)) : workspace.projects }))
        .filter((workspace) => workspace.projects.length > 0),
    [tree.workspaces, needle],
  )
  const visible = workspaces.flatMap((workspace) => workspace.projects.map((project) => project.gid))
  const count = selected.size
  const toggle = (ids: string[], on: boolean) =>
    setSelected((current) => {
      const next = new Set(current)
      for (const id of ids) {
        if (on) next.add(id)
        else next.delete(id)
      }
      return next
    })
  const all = checkState(visible, selected)
  return (
    <View className="flex-1">
      {tree.truncated ? (
        <WarningStrip>
          <WarningLine>This token can see more than 5,000 projects. Only the first 5,000 are listed.</WarningLine>
        </WarningStrip>
      ) : null}
      <ChooserToolbar query={query} onQuery={setQuery} placeholder="Filter projects" allLabel="Select all projects" {...all} disabled={visible.length === 0} onToggleAll={() => toggle(visible, !all.checked)} />
      <ScrollView className="flex-1" contentContainerClassName="p-2" keyboardShouldPersistTaps="handled">
        {workspaces.length === 0 ? (
          <Text className="p-4 text-[14px] leading-[20px] text-muted-foreground">{needle ? 'No project matches the filter.' : 'This token cannot see any project.'}</Text>
        ) : (
          workspaces.map((workspace) => (
            <View key={workspace.gid} accessibilityLabel={workspace.name} className="gap-1 pb-3">
              <Text accessibilityRole="header" className="px-2 pt-2 font-noto-semibold text-[12px] leading-[16px]">
                {workspace.name}
              </Text>
              {groupByTeam(workspace.projects).map((group) => {
                const ids = group.projects.map((project) => project.gid)
                const state = checkState(ids, selected)
                return (
                  <View key={group.team}>
                    <View className="h-8 flex-row items-center gap-2 px-2">
                      <Checkbox label={`Select team ${group.team}`} {...state} onPress={() => toggle(ids, !state.checked)} />
                      <Text numberOfLines={1} onPress={() => toggle(ids, !state.checked)} className="min-w-0 shrink font-noto-medium text-[12px] text-muted-foreground">
                        {group.team}
                      </Text>
                    </View>
                    {group.projects.map((project) => (
                      <ChoiceRow key={project.gid} indent label={project.name} checked={selected.has(project.gid)} onToggle={() => toggle([project.gid], !selected.has(project.gid))}>
                        <Text numberOfLines={1} className="min-w-0 flex-1 text-[13px]">
                          {project.name}
                        </Text>
                        {project.archived ? <Badge label="Archived" tone="outline" /> : null}
                      </ChoiceRow>
                    ))}
                  </View>
                )
              })}
            </View>
          ))
        )}
      </ScrollView>
      <ChooserFooter
        error={start.isError ? importErrorMessage('asana', start.error) : null}
        label={`Import ${count} ${count === 1 ? 'project' : 'projects'}`}
        disabled={count === 0 || start.isPending}
        onImport={() => start.mutate({ importId: item.id, body: { project_gids: [...selected] } })}
      >
        <Text className="min-w-0 flex-1 text-[12px] leading-[16px] text-muted-foreground">Each project becomes a new Orbit project with its sections, tasks, subtasks, comments, files and dependencies. Nobody is notified.</Text>
      </ChooserFooter>
    </View>
  )
}

function Result({ item }: { item: AsanaImport }) {
  const projects = useProjects()
  const report = item.report
  return (
    <ResultStep
      source={SOURCE}
      item={item}
      summary={item.error ? <Text className="text-[14px] leading-[20px] text-muted-foreground">{item.error}</Text> : null}
      stats={report ? [['Tasks imported', item.progress.done], ['Comments', report.comments_imported], ['Files', report.files_imported], ['Failed', item.progress.failed]] : null}
      columns={2}
      notes={importNotes('asana', item).map((note) => <NoteLine key={note}>{note}</NoteLine>)}
      notesTitle="Notes"
      failures={(report?.failures ?? []).map((failure) => ({ key: `${failure.kind}-${failure.asana_gid}`, title: `${failure.title.trim() ? failure.title : 'Untitled'}${failure.kind === 'project' ? ' (project)' : ''}`, error: failure.error }))}
      failuresTitle="Not imported"
    >
      <ImportedProjects ids={item.project_ids} projects={projects.data} />
    </ResultStep>
  )
}
