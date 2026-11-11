import type { ObsidianImport, ObsidianImportNode, ObsidianImportTree } from '@orbit/api/generated/types.gen'
import * as DocumentPicker from 'expo-document-picker'
import { router, useLocalSearchParams } from 'expo-router'
import { useMemo, useRef, useState } from 'react'
import { View } from 'react-native'

import { type FlowSource, ImportFlow, NoteLine, ResultStep } from '../../../../../components/import/Flow'
import { ChooserFooter, ConnectError, ConnectSection, DestinationFields, ImportBody, ImportedPages, ImportScreen, ImportTreePicker, NodeIcon, Paragraph, RecentImports, Strong } from '../../../../../components/import/Parts'
import { buildSelectionTree, destinationLabel, importErrorMessage, importNotes, importSelection, isImportInProgress, selectAll, selectedCount, skippedSummary, uploadObsidian } from '../../../../../data/import-api'
import { useImportDestination, useImports, useReconcileImport, useStartImport } from '../../../../../hooks/imports'
import { useOrbit } from '../../../../../orbit-context'
import { usePages, useTeamspaces } from '../../../../../queries'
import { useTheme } from '../../../../../theme'
import { Button, Icon, Spinner, Text } from '../../../../../ui'
import { uploadPart } from '../../../../../upload-part'

const HOME = '/settings/import/obsidian'

const SOURCE: FlowSource<'obsidian'> = {
  kind: 'obsidian',
  title: 'Import from Obsidian',
  chooseTitle: 'Choose notes',
  home: HOME,
  detail: (item) => item.file_name,
  scanning: (item) => ({ headline: `Reading “${item.file_name}”…`, text: 'Checking the archive and listing its notes. Large vaults take a minute.' }),
  cancel: {
    before: 'Orbit deletes the uploaded file. You can start a new import at any time.',
    started: 'Pages already imported stay. Empty pages the import created but did not fill yet are moved to the trash.',
  },
  importing: {
    headline: (item) => (item.status === 'queued' ? 'Waiting for the import to start…' : 'Importing notes…'),
    unit: 'pages',
    note: 'You can leave this page; the import continues on the server and new pages appear in the sidebar as they are created.',
  },
  treeMissing: 'The scanned note list is not available.',
}

/** The web's Obsidian import (`/docs/import/obsidian/:importId?`): upload the vault .zip, choose notes, follow it. */
export default function ObsidianImportScreen() {
  const { importId } = useLocalSearchParams<{ importId?: string }>()
  return <ImportFlow source={SOURCE} importId={importId} connect={<UploadStep />} chooser={(item, tree) => <Chooser item={item} tree={tree} />} result={(item) => <Result item={item} />} />
}

function UploadStep() {
  const { colors } = useTheme()
  const { client, workspaceId } = useOrbit()
  const imports = useImports('obsidian')
  const reconcile = useReconcileImport('obsidian')
  const [upload, setUpload] = useState<{ name: string; controller: AbortController } | null>(null)
  const [error, setError] = useState<{ message: string; inProgress: boolean } | null>(null)
  const busy = useRef(false)
  const running = imports.data?.find((item) => item.status === 'scanning' || item.status === 'queued' || item.status === 'importing')

  const choose = async () => {
    if (busy.current) return
    const picked = await DocumentPicker.getDocumentAsync({ type: ['application/zip', 'application/x-zip-compressed'], copyToCacheDirectory: true })
    const asset = picked.canceled ? undefined : picked.assets[0]
    if (!asset) return
    if (!asset.name.toLowerCase().endsWith('.zip')) {
      setError({ message: 'Upload the vault as one .zip file.', inProgress: false })
      return
    }
    busy.current = true
    const controller = new AbortController()
    setError(null)
    setUpload({ name: asset.name, controller })
    try {
      const file = await uploadPart(asset.uri, asset.name)
      const created = await uploadObsidian(client, workspaceId, file, asset.name, controller.signal)
      reconcile(created)
      router.replace(`${HOME}/${created.id}`)
    } catch (caught) {
      if (controller.signal.aborted) return
      setError({ message: importErrorMessage('obsidian', caught), inProgress: isImportInProgress(caught) })
      if (isImportInProgress(caught)) void imports.refetch()
    } finally {
      busy.current = false
      setUpload(null)
    }
  }

  return (
    <ImportScreen title={SOURCE.title}>
      <ImportBody>
        <ConnectSection
          title="Upload your vault"
          gap="gap-2"
          help="Zip the vault folder and upload the .zip file. Its folders become pages with their notes inside, and a note named like its folder (Projects/Projects.md) becomes that folder’s page. Images and files that notes link to or embed are attached to those pages. The .obsidian/ settings folder, canvases and CSV files are skipped. You choose which notes to import after the upload."
          intro={
            <Paragraph>
              To find the vault folder, open the vault switcher in Obsidian (bottom left) and choose <Strong>Reveal vault in system explorer</Strong>. Then right-click the folder and choose <Strong>Compress</Strong> (macOS) or <Strong>Compress to ZIP file</Strong> (Windows).
            </Paragraph>
          }
        >
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
            <View className="items-center gap-3 rounded-lg border border-dashed border-border px-4 py-8">
              <Icon name="documentUpload" size={24} color={colors.mutedForeground} />
              <Text className="text-center text-[14px] leading-[20px] text-muted-foreground">Choose the vault’s .zip file</Text>
              <Button label="Choose file" variant="outline" onPress={() => void choose()} />
            </View>
          )}
          {error ? <ConnectError message={error.message} running={error.inProgress && running ? `${HOME}/${running.id}` : null} /> : null}
        </ConnectSection>
        <RecentImports imports={imports.data} name={(item) => item.file_name} unit="pages" href={(item) => `${HOME}/${item.id}`} />
      </ImportBody>
    </ImportScreen>
  )
}

function Chooser({ item, tree }: { item: ObsidianImport; tree: ObsidianImportTree }) {
  const pages = usePages()
  const teamspaces = useTeamspaces()
  const index = useMemo(() => buildSelectionTree(tree.nodes, (node) => [node.path, node.parent_path]), [tree.nodes])
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => selectAll(index))
  const destination = useImportDestination(teamspaces.data, pages.data)
  const start = useStartImport('obsidian')
  const count = selectedCount(index, selected)
  const skipped = skippedSummary(tree.skipped)
  const submit = () => {
    const selection = importSelection(index, selected)
    if (!selection) return
    start.mutate({ importId: item.id, body: { selection: 'all' in selection ? selection : { paths: selection.roots }, destination: destination.body } })
  }
  return (
    <View className="flex-1">
      <View accessibilityLabel="Vault contents" className="gap-1 border-b border-border px-4 py-2">
        <Text className="text-[12px] leading-[16px] text-muted-foreground">
          {tree.attachments} {tree.attachments === 1 ? 'attachment' : 'attachments'} · {tree.unresolved_links} unresolved {tree.unresolved_links === 1 ? 'link' : 'links'} (kept as text) · {skipped.total} skipped {skipped.total === 1 ? 'file' : 'files'}
        </Text>
        {skipped.parts.length > 0 ? <Text className="text-[12px] leading-[16px] text-muted-foreground">Skipped: {skipped.parts.join(', ')}</Text> : null}
      </View>
      <ImportTreePicker
        tree={index}
        selected={selected}
        onSelectedChange={setSelected}
        label="Obsidian notes"
        empty="The vault has no notes to import."
        icon={(node: ObsidianImportNode) => <NodeIcon name={node.kind === 'folder' ? 'folder' : 'docs'} />}
      />
      <ChooserFooter error={start.isError ? importErrorMessage('obsidian', start.error) : null} label={`Import ${count} ${count === 1 ? 'page' : 'pages'}`} disabled={count === 0 || start.isPending} onImport={submit}>
        <DestinationFields destination={destination} />
      </ChooserFooter>
    </View>
  )
}

function Result({ item }: { item: ObsidianImport }) {
  const pages = usePages()
  const teamspaces = useTeamspaces()
  const report = item.report
  const failed = item.status === 'failed'
  const destination = destinationLabel(item.destination, teamspaces.data, pages.data)
  const skipped = report ? skippedSummary(report.skipped_files) : null
  const notes = [
    ...importNotes('obsidian', item).map((note) => <NoteLine key={note}>{note}</NoteLine>),
    ...(skipped && skipped.total > 0
      ? [
          <NoteLine key="skipped">
            Skipped files: <Text className="text-[14px] leading-[20px] text-muted-foreground">{skipped.parts.join(', ')}</Text>
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
          {item.error ? (
            failed ? (
              // a refused archive ends here with its reason
              <Text accessibilityRole="alert" className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-[14px] leading-[20px]">
                {item.error}
              </Text>
            ) : (
              <Text className="text-[14px] leading-[20px] text-muted-foreground">{item.error}</Text>
            )
          ) : null}
          {destination && item.progress.total > 0 ? <Text className="text-[14px] leading-[20px] text-muted-foreground">Destination: {destination}</Text> : null}
        </>
      }
      stats={item.progress.total > 0 ? [['Pages imported', item.progress.done], ['Files imported', report?.files_imported ?? 0], ['Pages failed', item.progress.failed]] : null}
      columns={1}
      notes={notes}
      notesTitle="Conversion notes"
      failures={(report?.failures ?? []).map((failure) => ({ key: failure.path, title: failure.title.trim() ? failure.title : failure.path, error: failure.error }))}
      failuresTitle="Failed pages"
      again={failed ? { label: 'Upload another file', primary: true } : undefined}
    >
      <ImportedPages rootIds={item.root_page_ids} pages={pages.data} />
    </ResultStep>
  )
}
