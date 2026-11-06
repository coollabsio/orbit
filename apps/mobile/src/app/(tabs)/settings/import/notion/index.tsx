import type { NotionImport, NotionImportNode, NotionImportTree } from '@orbit/api/generated/types.gen'
import { router, useLocalSearchParams } from 'expo-router'
import { useMemo, useState } from 'react'
import { View } from 'react-native'

import { type FlowSource, ImportFlow, NoteLine, ResultStep } from '../../../../../components/import/Flow'
import {
  ChooserFooter,
  ConnectError,
  ConnectSection,
  DestinationFields,
  ExternalLink,
  ImportBody,
  ImportedPages,
  ImportScreen,
  ImportTreePicker,
  NodeIcon,
  Paragraph,
  RecentImports,
  WarningLine,
  WarningStrip,
} from '../../../../../components/import/Parts'
import { buildSelectionTree, countList, createNotion, destinationLabel, importErrorMessage, importNotes, importSelection, isImportInProgress, selectAll, selectedCount } from '../../../../../data/import-api'
import { useConnectImport, useImportDestination, useStartImport } from '../../../../../hooks/imports'
import { useOrbit } from '../../../../../orbit-context'
import { usePages, useTeamspaces } from '../../../../../queries'
import { Button, Input, Text } from '../../../../../ui'

const TOKEN_URL = 'https://www.notion.so/developers/tokens'
const HOME = '/settings/import/notion'

const SOURCE: FlowSource<'notion'> = {
  kind: 'notion',
  title: 'Import from Notion',
  chooseTitle: 'Choose pages',
  home: HOME,
  detail: (item) => item.notion_workspace_name,
  scanning: (item) => ({
    headline: `Scanning ${item.notion_workspace_name ? `“${item.notion_workspace_name}”` : 'your Notion workspace'}…`,
    text: 'Listing every page the token can see. Large workspaces take a minute.',
  }),
  cancel: {
    before: 'Orbit deletes the Notion token. You can start a new import at any time.',
    started: 'Pages already imported stay. Empty pages the import created but did not fill yet are moved to the trash.',
  },
  importing: {
    headline: (item) => (item.status === 'queued' ? 'Waiting for the import to start…' : 'Importing pages…'),
    unit: 'pages',
    note: 'You can leave this page; the import continues on the server and new pages appear in the sidebar as they are created.',
  },
  treeMissing: 'The scanned page list is not available.',
}

/** The web's Notion import (`/docs/import/:importId?`): paste a token, choose pages and a destination, follow it. */
export default function NotionImportScreen() {
  const { importId } = useLocalSearchParams<{ importId?: string }>()
  return (
    <ImportFlow
      source={SOURCE}
      importId={importId}
      connect={<ConnectStep />}
      chooser={(item, tree) => <Chooser item={item} tree={tree} />}
      result={(item) => <Result item={item} />}
    />
  )
}

function ConnectStep() {
  const { client, workspaceId } = useOrbit()
  const connect = useConnectImport('notion', (created) => router.replace(`${HOME}/${created.id}`))
  const [token, setToken] = useState('')
  const value = token.trim()
  const submit = () => {
    if (!value) return
    void connect.submit(async () => {
      const created = await createNotion(client, workspaceId, value)
      setToken('')
      return created
    })
  }
  return (
    <ImportScreen title={SOURCE.title}>
      <ImportBody>
        <ConnectSection title="Connect Notion"
          intro={
            <Paragraph>
              Create a personal access token at <ExternalLink url={TOKEN_URL} /> (it can read everything you can see in Notion). Orbit keeps it encrypted only until the import finishes.
            </Paragraph>
          }
        >
          <View className="gap-3">
            <Input label="Notion token" secureTextEntry autoComplete="off" autoCapitalize="none" autoCorrect={false} placeholder="ntn_…" value={token} onChangeText={setToken} onSubmitEditing={submit} />
            {connect.error ? (
              <ConnectError message={importErrorMessage('notion', connect.error)} running={isImportInProgress(connect.error) && connect.running ? `${HOME}/${connect.running.id}` : null} />
            ) : null}
            <View className="flex-row">
              <Button label="Scan workspace" loading={connect.pending} disabled={!value} onPress={submit} />
            </View>
          </View>
        </ConnectSection>
        <RecentImports imports={connect.imports} name={(item) => item.notion_workspace_name || 'Notion'} unit="pages" href={(item) => `${HOME}/${item.id}`} />
      </ImportBody>
    </ImportScreen>
  )
}

function Chooser({ item, tree }: { item: NotionImport; tree: NotionImportTree }) {
  const pages = usePages()
  const teamspaces = useTeamspaces()
  const index = useMemo(() => buildSelectionTree(tree.nodes, (node) => [node.notion_id, node.parent_id]), [tree.nodes])
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => selectAll(index))
  const destination = useImportDestination(teamspaces.data, pages.data)
  const start = useStartImport('notion')
  const count = selectedCount(index, selected)
  const submit = () => {
    const selection = importSelection(index, selected)
    if (!selection) return
    start.mutate({ importId: item.id, body: { selection: 'all' in selection ? selection : { notion_ids: selection.roots }, destination: destination.body } })
  }
  return (
    <View className="flex-1">
      {tree.truncated || tree.incomplete ? (
        <WarningStrip>
          {tree.truncated ? <WarningLine>This token can see more than 5,000 pages. Only the first 5,000 are listed; import the rest in a later import.</WarningLine> : null}
          {tree.incomplete ? <WarningLine>Notion returned an incomplete list. Some pages may be missing; scan again later to find them.</WarningLine> : null}
        </WarningStrip>
      ) : null}
      <ImportTreePicker
        tree={index}
        selected={selected}
        onSelectedChange={setSelected}
        label="Notion pages"
        empty="This token cannot see any pages. In Notion, share pages with the connection (or use a personal access token) and scan again."
        icon={(node: NotionImportNode) => (node.icon ? <Text className="text-[13px]">{node.icon}</Text> : <NodeIcon name={node.kind === 'database' ? 'database' : 'docs'} />)}
      />
      <ChooserFooter error={start.isError ? importErrorMessage('notion', start.error) : null} label={`Import ${count} ${count === 1 ? 'page' : 'pages'}`} disabled={count === 0 || start.isPending} onImport={submit}>
        <DestinationFields destination={destination} />
      </ChooserFooter>
    </View>
  )
}

function Result({ item }: { item: NotionImport }) {
  const pages = usePages()
  const teamspaces = useTeamspaces()
  const report = item.report
  const destination = destinationLabel(item.destination, teamspaces.data, pages.data)
  const skipped = countList(report?.skipped ?? {})
  const lossy = countList(report?.lossy ?? {})
  const notes = [
    ...importNotes('notion', item).map((note) => <NoteLine key={note}>{note}</NoteLine>),
    ...(skipped
      ? [
          <NoteLine key="skipped">
            Skipped blocks (not supported in Orbit): <Text className="text-[14px] leading-[20px] text-muted-foreground">{skipped}</Text>
          </NoteLine>,
        ]
      : []),
    ...(lossy
      ? [
          <NoteLine key="lossy">
            Simplified blocks: <Text className="text-[14px] leading-[20px] text-muted-foreground">{lossy}</Text>
          </NoteLine>,
        ]
      : []),
  ]
  return (
    <ResultStep
      source={SOURCE}
      item={item}
      summary={
        <>
          {item.error ? <Text className="text-[14px] leading-[20px] text-muted-foreground">{item.error}</Text> : null}
          {destination && item.progress.total > 0 ? <Text className="text-[14px] leading-[20px] text-muted-foreground">Destination: {destination}</Text> : null}
        </>
      }
      stats={item.progress.total > 0 ? [['Pages imported', item.progress.done], ['Files imported', report?.files_imported ?? 0], ['Pages failed', item.progress.failed]] : null}
      columns={1}
      notes={notes}
      notesTitle="Conversion notes"
      failures={(report?.failures ?? []).map((failure) => ({ key: failure.notion_id, title: failure.title.trim() ? failure.title : 'Untitled', error: failure.error }))}
      failuresTitle="Failed pages"
    >
      <ImportedPages rootIds={item.root_page_ids} pages={pages.data} />
    </ResultStep>
  )
}
