import type { LinearImport, LinearImportTree } from '@orbit/api/generated/types.gen'
import { router, useLocalSearchParams } from 'expo-router'
import { useMemo, useState } from 'react'
import { ScrollView, View } from 'react-native'

import { type FlowSource, ImportFlow, NoteLine, ResultStep } from '../../../../../components/import/Flow'
import { ChoiceRow, ChooserFooter, ChooserToolbar, ConnectError, ConnectSection, ExternalLink, ImportBody, ImportedProjects, ImportScreen, Paragraph, RecentImports, WarningLine, WarningStrip } from '../../../../../components/import/Parts'
import { Badge } from '../../../../../components/settings/Shell'
import { createLinear, importErrorMessage, importNotes, isImportInProgress, LINEAR_IMPORT_MAX_ISSUES, linearImportProgress, linearRateLimitNotice, plural } from '../../../../../data/import-api'
import { useConnectImport, useStartImport } from '../../../../../hooks/imports'
import { useOrbit } from '../../../../../orbit-context'
import { useProjects } from '../../../../../queries'
import { Button, Input, Text } from '../../../../../ui'

const KEY_URL = 'https://linear.app/settings/account/security'
const HOME = '/settings/import/linear'

const SOURCE: FlowSource<'linear'> = {
  kind: 'linear',
  title: 'Import from Linear',
  chooseTitle: 'Choose teams',
  home: HOME,
  detail: (item) => item.linear_user_name,
  scanning: () => ({ headline: 'Finding your Linear teams…', text: 'Listing every team the key can see.' }),
  cancel: {
    before: 'Orbit deletes the Linear API key. You can start a new import at any time.',
    started: 'Projects and issues already imported stay. The import stops before the next issue.',
  },
  importing: {
    headline: (item) => linearImportProgress(item).label,
    unit: 'issues',
    progress: linearImportProgress,
    notice: { until: (item) => item.waiting_until, text: linearRateLimitNotice },
    note: 'You can leave this page; the import continues on the server. Linear allows a limited number of requests per hour, so large teams take a while.',
  },
  treeMissing: 'The team list is not available.',
}

/** The web's Linear import (`/tasks-import/linear/:importId?`): paste an API key, choose teams, follow the import. */
export default function LinearImportScreen() {
  const { importId } = useLocalSearchParams<{ importId?: string }>()
  return <ImportFlow source={SOURCE} importId={importId} connect={<ConnectStep />} chooser={(item, tree) => <Chooser item={item} tree={tree} />} result={(item) => <Result item={item} />} />
}

function ConnectStep() {
  const { client, workspaceId } = useOrbit()
  const connect = useConnectImport('linear', (created) => router.replace(`${HOME}/${created.id}`))
  const [key, setKey] = useState('')
  const value = key.trim()
  const submit = () => {
    if (!value) return
    void connect.submit(async () => {
      const created = await createLinear(client, workspaceId, value)
      setKey('')
      return created
    })
  }
  return (
    <ImportScreen title={SOURCE.title}>
      <ImportBody>
        <ConnectSection title="Connect Linear"
          intro={
            <Paragraph>
              Create a personal API key at <ExternalLink url={KEY_URL} /> (with read access it can read everything you can see in Linear). Orbit keeps it encrypted only until the import finishes.
            </Paragraph>
          }
        >
          <View className="gap-3">
            <Input label="Linear personal API key" secureTextEntry autoComplete="off" autoCapitalize="none" autoCorrect={false} placeholder="lin_api_…" value={key} onChangeText={setKey} onSubmitEditing={submit} />
            {connect.error ? <ConnectError message={importErrorMessage('linear', connect.error)} running={isImportInProgress(connect.error) && connect.running ? `${HOME}/${connect.running.id}` : null} /> : null}
            <View className="flex-row">
              <Button label="Find teams" loading={connect.pending} disabled={!value} onPress={submit} />
            </View>
          </View>
        </ConnectSection>
        <RecentImports imports={connect.imports} name={(item) => item.linear_user_name || 'Linear'} unit="issues" href={(item) => `${HOME}/${item.id}`} />
      </ImportBody>
    </ImportScreen>
  )
}

/** Pick teams. Active teams start selected, archived ones do not. */
function Chooser({ item, tree }: { item: LinearImport; tree: LinearImportTree }) {
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set(tree.teams.filter((team) => !team.archived).map((team) => team.id)))
  const [query, setQuery] = useState('')
  const start = useStartImport('linear')
  const needle = query.trim().toLowerCase()
  const teams = useMemo(() => (needle ? tree.teams.filter((team) => `${team.name} ${team.key}`.toLowerCase().includes(needle)) : tree.teams), [tree.teams, needle])
  const visible = teams.map((team) => team.id)
  const count = selected.size
  const issues = tree.teams.filter((team) => selected.has(team.id)).reduce((sum, team) => sum + team.issue_count, 0)
  const toggle = (ids: string[], on: boolean) =>
    setSelected((current) => {
      const next = new Set(current)
      for (const id of ids) {
        if (on) next.add(id)
        else next.delete(id)
      }
      return next
    })
  const chosen = visible.filter((id) => selected.has(id)).length
  const checked = chosen > 0 && chosen === visible.length
  return (
    <View className="flex-1">
      {tree.truncated ? (
        <WarningStrip>
          <WarningLine>This key can see more than 1,000 teams. Only the first 1,000 are listed.</WarningLine>
        </WarningStrip>
      ) : null}
      <ChooserToolbar query={query} onQuery={setQuery} placeholder="Filter teams" allLabel="Select all teams" checked={checked} indeterminate={chosen > 0 && !checked} disabled={visible.length === 0} onToggleAll={() => toggle(visible, !checked)} />
      <ScrollView className="flex-1" contentContainerClassName="p-2" keyboardShouldPersistTaps="handled">
        {teams.length === 0 ? (
          <Text className="p-4 text-[14px] leading-[20px] text-muted-foreground">{needle ? 'No team matches the filter.' : 'This key cannot see any team.'}</Text>
        ) : (
          <View accessibilityLabel={tree.organization} className="gap-1">
            <Text accessibilityRole="header" className="px-2 pt-2 pb-1 font-noto-semibold text-[12px] leading-[16px]">
              {tree.organization}
            </Text>
            {teams.map((team) => (
              <ChoiceRow key={team.id} label={team.name} checked={selected.has(team.id)} onToggle={() => toggle([team.id], !selected.has(team.id))}>
                <Text numberOfLines={1} className="min-w-0 flex-1 text-[13px]">
                  {team.name}
                  {team.key ? <Text className="text-[13px] text-muted-foreground"> · {team.key}</Text> : null}
                </Text>
                {team.archived ? <Badge label="Archived" tone="outline" /> : null}
                <Text className="text-[12px] text-muted-foreground tabular-nums">{plural(team.issue_count, 'issue', 'issues')}</Text>
              </ChoiceRow>
            ))}
          </View>
        )}
      </ScrollView>
      <ChooserFooter
        error={
          start.isError
            ? importErrorMessage('linear', start.error)
            : issues > LINEAR_IMPORT_MAX_ISSUES
              ? `These teams have ${issues.toLocaleString('en-US')} issues. One import takes at most ${LINEAR_IMPORT_MAX_ISSUES.toLocaleString('en-US')}: import fewer teams at a time.`
              : null
        }
        label={`Import ${plural(count, 'team', 'teams')}`}
        disabled={count === 0 || start.isPending}
        onImport={() => start.mutate({ importId: item.id, body: { team_ids: [...selected] } })}
      >
        <Text className="min-w-0 flex-1 text-[12px] leading-[16px] text-muted-foreground">
          Each team becomes a new Orbit project with its workflow states, issues, sub-issues, labels, comments, files and relations. Nobody is notified.
        </Text>
      </ChooserFooter>
    </View>
  )
}

function Result({ item }: { item: LinearImport }) {
  const projects = useProjects()
  const report = item.report
  return (
    <ResultStep
      source={SOURCE}
      item={item}
      summary={item.error ? <Text className="text-[14px] leading-[20px] text-muted-foreground">{item.error}</Text> : null}
      stats={report ? [['Issues imported', item.progress.done], ['Comments', report.comments_imported], ['Files', report.files_imported], ['Failed', item.progress.failed]] : null}
      columns={2}
      notes={importNotes('linear', item).map((note) => <NoteLine key={note}>{note}</NoteLine>)}
      notesTitle="Notes"
      failures={(report?.failures ?? []).map((failure) => ({ key: `${failure.kind}-${failure.linear_id}`, title: `${failure.title.trim() ? failure.title : 'Untitled'}${failure.kind === 'issue' ? '' : ` (${failure.kind})`}`, error: failure.error }))}
      failuresTitle="Not imported"
    >
      <ImportedProjects ids={item.project_ids} projects={projects.data} />
    </ResultStep>
  )
}
