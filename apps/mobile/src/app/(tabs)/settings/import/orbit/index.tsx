import type { OrbitImport, OrbitImportTree } from '@orbit/api/generated/types.gen'
import { CHANNEL_GROUPS, fullSelection, orbitImportName, orbitImportPhaseText } from '@orbit/api/orbitImports'
import * as Clipboard from 'expo-clipboard'
import * as DocumentPicker from 'expo-document-picker'
import { router, useLocalSearchParams } from 'expo-router'
import { useRef, useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import { type FlowSource, ImportFlow, NoteLine, ResultStep } from '../../../../../components/import/Flow'
import { Checkbox, ChoiceRow, ChooserFooter, ConnectError, ConnectSection, ImportBody, ImportScreen, Paragraph, RecentImports, Strong } from '../../../../../components/import/Parts'
import { createOrbit, importErrorMessage, importNotes, isImportInProgress, plural, uploadOrbit } from '../../../../../data/import-api'
import { useConnectImport, useStartImport } from '../../../../../hooks/imports'
import { useOrbit } from '../../../../../orbit-context'
import { useTheme } from '../../../../../theme'
import { Button, Icon, Input, Spinner, Text, LabelTip } from '../../../../../ui'
import { uploadPart } from '../../../../../upload-part'

const HOME = '/settings/import/orbit'

const SOURCE: FlowSource<'orbit'> = {
  kind: 'orbit',
  title: 'Import from Orbit',
  chooseTitle: 'Choose what to import',
  home: HOME,
  detail: orbitImportName,
  scanning: orbitImportPhaseText,
  cancel: {
    before: 'Orbit deletes the token and the export. You can start a new import at any time.',
    started: 'What was already imported stays. The import stops before the next item.',
  },
  importing: {
    headline: (item) => (item.status === 'queued' ? 'Waiting for the import to start…' : 'Importing…'),
    unit: 'items',
    note: 'You can leave this page; the import continues on the server.',
  },
  treeMissing: 'The export contents are not available.',
}

/** The web's Orbit import (`/settings/import/orbit/:importId?`): connect or upload, choose, follow the import. */
export default function OrbitImportScreen() {
  const { importId } = useLocalSearchParams<{ importId?: string }>()
  return <ImportFlow source={SOURCE} importId={importId} connect={<ConnectStep />} chooser={(item, tree) => <Chooser item={item} tree={tree} />} result={(item) => <Result item={item} />} />
}

function ConnectStep() {
  const { colors } = useTheme()
  const { client, workspaceId } = useOrbit()
  const connect = useConnectImport('orbit', (created) => router.replace(`${HOME}/${created.id}`))
  const [url, setUrl] = useState('')
  const [token, setToken] = useState('')
  const [upload, setUpload] = useState<{ name: string; controller: AbortController } | null>(null)
  const busy = useRef(false)
  const [origin, setOrigin] = useState<'url' | 'upload'>('url')
  const ready = Boolean(url.trim() && token.trim())
  const running = isImportInProgress(connect.error) && connect.running ? `${HOME}/${connect.running.id}` : null

  const submit = () => {
    if (!ready) return
    setOrigin('url')
    void connect.submit(async () => {
      const created = await createOrbit(client, workspaceId, url.trim(), token.trim())
      setToken('')
      return created
    })
  }

  const choose = async () => {
    if (busy.current) return
    const picked = await DocumentPicker.getDocumentAsync({ type: ['application/zip', 'application/x-zip-compressed'], copyToCacheDirectory: true })
    const asset = picked.canceled ? undefined : picked.assets[0]
    if (!asset) return
    busy.current = true
    setOrigin('upload')
    const controller = new AbortController()
    setUpload({ name: asset.name, controller })
    try {
      await connect.submit(async () => uploadOrbit(client, workspaceId, await uploadPart(asset.uri, asset.name), asset.name, controller.signal))
    } finally {
      if (controller.signal.aborted) connect.setError(null)
      busy.current = false
      setUpload(null)
    }
  }

  return (
    <ImportScreen title={SOURCE.title}>
      <ImportBody>
        <ConnectSection
          title="Connect another Orbit"
          help="Copy projects, pages and chats from another Orbit instance into this workspace. Nobody is notified."
          intro={
            <Paragraph>
              On the other instance, open <Strong>Settings → API tokens</Strong> and create a token with <Strong>Workspace export</Strong> (owners and administrators: the whole workspace) or <Strong>Personal export</Strong> (any member: your private pages, channels and direct messages). Orbit keeps the token encrypted only until the export is downloaded.
            </Paragraph>
          }
        >
          <View className="gap-3">
            <Input label="Instance address" keyboardType="url" autoComplete="off" autoCapitalize="none" autoCorrect={false} placeholder="https://orbit.example.com" value={url} onChangeText={setUrl} />
            <Input label="API token" secureTextEntry autoComplete="off" autoCapitalize="none" autoCorrect={false} value={token} onChangeText={setToken} onSubmitEditing={submit} />
            {connect.error && origin === 'url' ? <ConnectError message={importErrorMessage('orbit', connect.error)} running={running} /> : null}
            <View className="flex-row">
              <Button label="Connect" loading={connect.pending && origin === 'url'} disabled={!ready || connect.pending} onPress={submit} />
            </View>
          </View>
        </ConnectSection>
        <ConnectSection title="Or upload an export" help="Download an export ZIP on the other instance in Settings → Export and upload it here.">
          {upload ? (
            <View accessibilityLiveRegion="polite" className="gap-2">
              <View className="flex-row items-center gap-2">
                <Spinner />
                <Text numberOfLines={1} className="min-w-0 flex-1 text-[14px] leading-[20px]">
                  Uploading {upload.name}…
                </Text>
              </View>
              <View className="flex-row">
                <Button label="Cancel upload" variant="outline" size="sm" onPress={() => upload.controller.abort()} />
              </View>
            </View>
          ) : (
            <View className="flex-row">
              <Button label="Choose file" icon="documentUpload" variant="outline" disabled={connect.pending} onPress={() => void choose()} />
            </View>
          )}
          {connect.error && origin === 'upload' ? <ConnectError message={importErrorMessage('orbit', connect.error)} running={running} /> : null}
        </ConnectSection>
        <RecentImports imports={connect.imports} name={orbitImportName} unit="items" href={(item) => `${HOME}/${item.id}`} />
      </ImportBody>
    </ImportScreen>
  )
}

type Choice = { id: string; name: string; detail?: string; count: string }

/** One group of rows with a "select all" box in its header. */
function ChoiceGroup({ title, choices, selected, onChange }: { title: string; choices: Choice[]; selected: ReadonlySet<string>; onChange: (next: ReadonlySet<string>) => void }) {
  if (choices.length === 0) return null
  const chosen = choices.filter((choice) => selected.has(choice.id)).length
  const all = chosen === choices.length
  const toggle = (ids: string[], on: boolean) => {
    const next = new Set(selected)
    for (const id of ids) {
      if (on) next.add(id)
      else next.delete(id)
    }
    onChange(next)
  }
  const toggleAll = () => toggle(choices.map((choice) => choice.id), !all)
  return (
    <View accessibilityLabel={title} className="gap-1">
      <Pressable onPress={toggleAll} className="h-8 flex-row items-center gap-2 px-2">
        <Checkbox label={`Select all ${title.toLowerCase()}`} checked={all} indeterminate={chosen > 0 && !all} onPress={toggleAll} />
        <Text accessibilityRole="header" className="font-noto-semibold text-[12px] leading-[16px]">
          {title}
        </Text>
      </Pressable>
      {choices.map((choice) => (
        <ChoiceRow key={choice.id} indent label={choice.name} checked={selected.has(choice.id)} onToggle={() => toggle([choice.id], !selected.has(choice.id))}>
          <Text numberOfLines={1} className="min-w-0 flex-1 text-[13px]">
            {choice.name}
            {choice.detail ? <Text className="text-[13px] text-muted-foreground"> · {choice.detail}</Text> : null}
          </Text>
          <Text className="text-[12px] text-muted-foreground tabular-nums">{choice.count}</Text>
        </ChoiceRow>
      ))}
    </View>
  )
}

/** Everything starts selected. A personal import has only private pages and channels. */
function Chooser({ item, tree }: { item: OrbitImport; tree: OrbitImportTree }) {
  const full = item.mode === 'full'
  const [initial] = useState(() => fullSelection(item.mode, tree))
  const [projects, setProjects] = useState<ReadonlySet<string>>(() => new Set(initial.project_ids))
  const [teamspaces, setTeamspaces] = useState<ReadonlySet<string>>(() => new Set(initial.teamspace_ids))
  const [privatePages, setPrivatePages] = useState(initial.include_private_pages)
  const [channels, setChannels] = useState<ReadonlySet<string>>(() => new Set(initial.channel_ids))
  const start = useStartImport('orbit')
  const selection = {
    project_ids: full ? [...projects] : [],
    teamspace_ids: full ? [...teamspaces] : [],
    include_private_pages: privatePages,
    channel_ids: [...channels],
  }
  const empty = !selection.project_ids.length && !selection.teamspace_ids.length && !privatePages && !selection.channel_ids.length
  const nothing = (full ? tree.projects.length + tree.teamspaces.length : 0) + tree.private_page_count + tree.channels.length === 0
  return (
    <View className="flex-1">
      <ScrollView className="flex-1" contentContainerClassName="gap-3 p-2">
        {nothing ? <Text className="p-2 text-[14px] leading-[20px] text-muted-foreground">This export has nothing to import.</Text> : null}
        {full ? (
          <>
            <ChoiceGroup title="Projects" selected={projects} onChange={setProjects} choices={tree.projects.map((project) => ({ id: project.id, name: project.name, detail: project.key, count: plural(project.task_count, 'task', 'tasks') }))} />
            <ChoiceGroup title="Teamspaces" selected={teamspaces} onChange={setTeamspaces} choices={tree.teamspaces.map((teamspace) => ({ id: teamspace.id, name: teamspace.name, count: plural(teamspace.page_count, 'page', 'pages') }))} />
          </>
        ) : null}
        {tree.private_page_count > 0 ? (
          <ChoiceRow label="my private pages" checked={privatePages} onToggle={() => setPrivatePages(!privatePages)}>
            <Text className="min-w-0 flex-1 font-noto-semibold text-[12px] leading-[16px]">My private pages</Text>
            <Text className="text-[12px] text-muted-foreground tabular-nums">{plural(tree.private_page_count, 'page', 'pages')}</Text>
          </ChoiceRow>
        ) : null}
        {CHANNEL_GROUPS.map((group) => (
          <ChoiceGroup
            key={group.kind}
            title={group.label}
            selected={channels}
            onChange={setChannels}
            choices={tree.channels
              .filter((channel) => channel.kind === group.kind)
              .map((channel) => ({ id: channel.id, name: group.kind === 'dm' ? channel.name : `#${channel.name}`, count: plural(channel.message_count, 'message', 'messages') }))}
          />
        ))}
      </ScrollView>
      <ChooserFooter error={start.isError ? importErrorMessage('orbit', start.error) : null} label="Import" disabled={empty || start.isPending} onImport={() => start.mutate({ importId: item.id, body: { selection } })}>
        <Text className="min-w-0 flex-1 text-[12px] leading-[16px] text-muted-foreground">
          {full
            ? 'Projects keep their tasks, comments and files; teamspaces keep their pages. Items imported before are skipped. Nobody is notified.'
            : 'Your private pages stay private, and channels keep their messages and files. Items imported before are skipped. Nobody is notified.'}
        </Text>
      </ChooserFooter>
    </View>
  )
}

function Result({ item }: { item: OrbitImport }) {
  const { colors } = useTheme()
  const { session } = useOrbit()
  const [copied, setCopied] = useState(false)
  const report = item.report
  const created = report?.created
  return (
    <ResultStep
      source={SOURCE}
      item={item}
      summary={item.error ? <Text className="text-[14px] leading-[20px] text-muted-foreground">{item.error}</Text> : null}
      stats={created ? [['Projects', created.projects ?? 0], ['Tasks', created.tasks ?? 0], ['Pages', created.pages ?? 0], ['Channels', created.channels ?? 0], ['Messages', created.messages ?? 0], ['Files', created.files ?? 0]] : null}
      columns={2}
      notes={importNotes('orbit', item).map((note) => <NoteLine key={note}>{note}</NoteLine>)}
      notesTitle="Notes"
      failures={(report?.failures ?? []).map((failure, index) => ({ key: `${failure.kind}-${index}`, title: `${failure.title.trim() ? failure.title : 'Untitled'} (${failure.kind})`, error: failure.error }))}
      failuresTitle="Not imported"
    >
      {report && report.unmatched_users.length > 0 ? (
        <View accessibilityLabel="People with no member here" className="gap-2">
          <LabelTip label="People with no member here" text="No member here has their email, so their tasks are unassigned." className="font-noto-medium text-[12px] leading-[16px] text-muted-foreground" />
          <View className="border-t border-border">
            {report.unmatched_users.map((user) => (
              <View key={user.email ?? user.name} className="flex-row items-center gap-3 border-b border-border px-1 py-2">
                <Text numberOfLines={1} className="min-w-0 flex-1 text-[13px]">
                  {user.name}
                </Text>
                <Text numberOfLines={1} className="min-w-0 shrink text-[13px] text-muted-foreground">
                  {user.email}
                </Text>
              </View>
            ))}
          </View>
        </View>
      ) : null}
      {report?.members_must_import_private && item.status === 'completed' ? (
        <View accessibilityLabel="Private data" className="gap-2 rounded-lg border border-border p-3">
          <Text className="text-[14px] leading-[20px]">Other members’ private pages and chats are not included. Each member must import them with their own token.</Text>
          <View className="flex-row items-center gap-2">
            <Button label={copied ? 'Copied' : 'Copy link for members'} icon="copy" variant="outline" size="sm" onPress={() => void Clipboard.setStringAsync(`${session.server}${HOME}`).then(setCopied, () => setCopied(false))} />
            {copied ? <Icon name="check" size={16} color={colors.primary} /> : null}
          </View>
        </View>
      ) : null}
    </ResultStep>
  )
}
