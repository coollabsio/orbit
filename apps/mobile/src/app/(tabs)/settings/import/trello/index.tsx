import type { TrelloImport, TrelloImportTree } from '@orbit/api/generated/types.gen'
import { router, useLocalSearchParams } from 'expo-router'
import * as WebBrowser from 'expo-web-browser'
import { type ReactNode, useMemo, useState } from 'react'
import { ScrollView, View } from 'react-native'

import { type FlowSource, ImportFlow, NoteLine, ResultStep } from '../../../../../components/import/Flow'
import { ChoiceRow, ChooserFooter, ChooserToolbar, ConnectError, ConnectSection, ExternalLink, ImportBody, ImportedProjects, ImportScreen, Paragraph, RecentImports, Strong, WarningLine, WarningStrip } from '../../../../../components/import/Parts'
import { Badge } from '../../../../../components/settings/Shell'
import { createTrello, importErrorMessage, importNotes, isImportInProgress, plural, trelloCredentialField, trelloTokenUrl } from '../../../../../data/import-api'
import { useConnectImport, useStartImport } from '../../../../../hooks/imports'
import { useOrbit } from '../../../../../orbit-context'
import { useProjects } from '../../../../../queries'
import { Button, Input, Text } from '../../../../../ui'

const POWER_UP_ADMIN_URL = 'https://trello.com/apps/admin'
const HOME = '/settings/import/trello'

const SOURCE: FlowSource<'trello'> = {
  kind: 'trello',
  title: 'Import from Trello',
  chooseTitle: 'Choose boards',
  home: HOME,
  detail: (item) => item.trello_user_name,
  scanning: () => ({ headline: 'Finding your Trello boards…', text: 'Listing every board the token can see.' }),
  cancel: {
    before: 'Orbit deletes the Trello key and token. You can start a new import at any time.',
    started: 'Projects and cards already imported stay. The import stops before the next card.',
  },
  importing: {
    headline: (item) => (item.status === 'queued' ? 'Waiting for the import to start…' : item.progress.total === 0 ? 'Reading boards…' : 'Importing cards…'),
    unit: 'cards',
    note: 'You can leave this page; the import continues on the server. Trello limits how fast Orbit can read, so large boards take a while.',
  },
  treeMissing: 'The board list is not available.',
}

/** The web's Trello import (`/tasks-import/trello/:importId?`): paste a Power-Up key and a token, choose boards. */
export default function TrelloImportScreen() {
  const { importId } = useLocalSearchParams<{ importId?: string }>()
  return <ImportFlow source={SOURCE} importId={importId} connect={<ConnectStep />} chooser={(item, tree) => <Chooser item={item} tree={tree} />} result={(item) => <Result item={item} />} />
}

/** One numbered step of the web's ordered list. */
function Step({ number, children }: { number: number; children: ReactNode }) {
  return (
    <View className="flex-row">
      <Text className="w-5 text-[14px] leading-[20px] text-muted-foreground">{number}.</Text>
      <Text className="flex-1 text-[14px] leading-[20px] text-muted-foreground">{children}</Text>
    </View>
  )
}

function ConnectStep() {
  const { client, workspaceId } = useOrbit()
  const connect = useConnectImport('trello', (created) => router.replace(`${HOME}/${created.id}`))
  const [key, setKey] = useState('')
  const [token, setToken] = useState('')
  const field = trelloCredentialField(connect.error)
  const trimmedKey = key.trim()
  const ready = trimmedKey !== '' && token.trim() !== ''
  const submit = () => {
    if (!ready) return
    const credentials = { key: trimmedKey, token: token.trim() }
    void connect.submit(async () => {
      const created = await createTrello(client, workspaceId, credentials)
      setKey('')
      setToken('')
      return created
    })
  }
  const fieldError = (name: 'key' | 'token') => (field === name ? importErrorMessage('trello', connect.error) : undefined)
  const plain = { secureTextEntry: true, autoComplete: 'off', autoCapitalize: 'none', autoCorrect: false } as const
  return (
    <ImportScreen title={SOURCE.title}>
      <ImportBody>
        <ConnectSection title="Connect Trello" intro={<Paragraph>Orbit needs an API key and a token. It keeps both encrypted only until the import finishes.</Paragraph>}>
          <View className="gap-4">
            <View className="gap-1.5">
              <Text className="font-noto-medium text-[14px] leading-[19.25px]">API key</Text>
              <View className="gap-0.5">
                <Step number={1}>
                  Open the <ExternalLink url={POWER_UP_ADMIN_URL}>Trello Power-Up admin page</ExternalLink> and click <Strong>New</Strong>.
                </Step>
                <Step number={2}>
                  Choose <Strong>My app doesn’t use Power-up capabilities</Strong>, type a name (for example “Orbit import”), pick your Trello workspace, fill in your email, and click <Strong>Create</Strong>.
                </Step>
                <Step number={3}>
                  Under <Strong>Authorization</Strong>, open <Strong>Trello Auth</Strong>, generate an API key if there is none, and copy the key.
                </Step>
              </View>
              <Input
                accessibilityLabel="API key"
                {...plain}
                value={key}
                error={fieldError('key')}
                onChangeText={(value) => {
                  setKey(value)
                  if (field === 'key') connect.setError(null)
                }}
              />
            </View>
            <View className="gap-1.5">
              <Text className="font-noto-medium text-[14px] leading-[19.25px]">Token</Text>
              <Paragraph>
                {trimmedKey ? (
                  <Text accessibilityRole="link" onPress={() => void WebBrowser.openBrowserAsync(trelloTokenUrl(trimmedKey)).catch(() => undefined)} className="text-[14px] leading-[20px] text-primary underline">
                    Generate a token
                  </Text>
                ) : (
                  'Generate a token (paste the API key first)'
                )}{' '}
                and click Allow. Trello then shows the token. It only gives read access and expires in a day.
              </Paragraph>
              <Input
                accessibilityLabel="Token"
                {...plain}
                value={token}
                error={fieldError('token')}
                onSubmitEditing={submit}
                onChangeText={(value) => {
                  setToken(value)
                  if (field === 'token') connect.setError(null)
                }}
              />
            </View>
            {connect.error && !field ? <ConnectError message={importErrorMessage('trello', connect.error)} running={isImportInProgress(connect.error) && connect.running ? `${HOME}/${connect.running.id}` : null} /> : null}
            <View className="flex-row">
              <Button label="Find boards" loading={connect.pending} disabled={!ready} onPress={submit} />
            </View>
          </View>
        </ConnectSection>
        <RecentImports imports={connect.imports} name={(item) => item.trello_user_name || 'Trello'} unit="cards" href={(item) => `${HOME}/${item.id}`} />
      </ImportBody>
    </ImportScreen>
  )
}

/** Pick boards, grouped by Trello Workspace. Open boards start selected, archived ones do not. */
function Chooser({ item, tree }: { item: TrelloImport; tree: TrelloImportTree }) {
  const boards = useMemo(() => tree.workspaces.flatMap((group) => group.boards), [tree.workspaces])
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set(boards.filter((board) => !board.archived).map((board) => board.id)))
  const [query, setQuery] = useState('')
  const start = useStartImport('trello')
  const needle = query.trim().toLowerCase()
  const groups = useMemo(
    () => tree.workspaces.map((group) => ({ ...group, boards: needle ? group.boards.filter((board) => board.name.toLowerCase().includes(needle)) : group.boards })).filter((group) => group.boards.length > 0),
    [tree.workspaces, needle],
  )
  const visible = groups.flatMap((group) => group.boards.map((board) => board.id))
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
  const chosen = visible.filter((id) => selected.has(id)).length
  const checked = chosen > 0 && chosen === visible.length
  return (
    <View className="flex-1">
      {tree.truncated ? (
        <WarningStrip>
          <WarningLine>This token can see more than 1,000 boards. Only the first 1,000 are listed.</WarningLine>
        </WarningStrip>
      ) : null}
      <ChooserToolbar query={query} onQuery={setQuery} placeholder="Filter boards" allLabel="Select all boards" checked={checked} indeterminate={chosen > 0 && !checked} disabled={visible.length === 0} onToggleAll={() => toggle(visible, !checked)} />
      <ScrollView className="flex-1" contentContainerClassName="p-2" keyboardShouldPersistTaps="handled">
        {groups.length === 0 ? (
          <Text className="p-4 text-[14px] leading-[20px] text-muted-foreground">{needle ? 'No board matches the filter.' : 'This token cannot see any board.'}</Text>
        ) : (
          groups.map((group) => {
            const heading = group.id === null ? 'Other boards' : group.name
            return (
              <View key={group.id ?? 'other'} accessibilityLabel={heading} className="gap-1">
                <Text accessibilityRole="header" className="px-2 pt-2 pb-1 font-noto-semibold text-[12px] leading-[16px]">
                  {heading}
                </Text>
                {group.boards.map((board) => (
                  <ChoiceRow key={board.id} label={board.name} checked={selected.has(board.id)} onToggle={() => toggle([board.id], !selected.has(board.id))}>
                    <Text numberOfLines={1} className="min-w-0 flex-1 text-[13px]">
                      {board.name}
                    </Text>
                    {board.archived ? <Badge label="Archived" tone="outline" /> : null}
                    {board.card_count === null ? null : <Text className="text-[12px] text-muted-foreground tabular-nums">{plural(board.card_count, 'card', 'cards')}</Text>}
                  </ChoiceRow>
                ))}
              </View>
            )
          })
        )}
      </ScrollView>
      <ChooserFooter
        error={start.isError ? importErrorMessage('trello', start.error) : null}
        label={`Import ${plural(count, 'board', 'boards')}`}
        disabled={count === 0 || start.isPending}
        onImport={() => start.mutate({ importId: item.id, body: { board_ids: [...selected] } })}
      >
        <Text className="min-w-0 flex-1 text-[12px] leading-[16px] text-muted-foreground">
          Each board becomes a new Orbit project. Its open lists become statuses, and its open cards become tasks with their labels, checklists, comments and files. Nobody is notified.
        </Text>
      </ChooserFooter>
    </View>
  )
}

function Result({ item }: { item: TrelloImport }) {
  const projects = useProjects()
  const report = item.report
  return (
    <ResultStep
      source={SOURCE}
      item={item}
      summary={item.error ? <Text className="text-[14px] leading-[20px] text-muted-foreground">{item.error}</Text> : null}
      stats={report ? [['Cards imported', item.progress.done], ['Comments', report.comments_imported], ['Files', report.files_imported], ['Failed', item.progress.failed]] : null}
      columns={2}
      notes={importNotes('trello', item).map((note) => <NoteLine key={note}>{note}</NoteLine>)}
      notesTitle="Notes"
      failures={(report?.failures ?? []).map((failure) => ({ key: `${failure.kind}-${failure.trello_id}`, title: `${failure.title.trim() ? failure.title : 'Untitled'}${failure.kind === 'card' ? '' : ` (${failure.kind})`}`, error: failure.error }))}
      failuresTitle="Not imported"
    >
      <ImportedProjects ids={item.project_ids} projects={projects.data} />
    </ResultStep>
  )
}
