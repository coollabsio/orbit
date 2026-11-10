/**
 * The files of a row (spec P3 12.2), the web's files popover at phone size: each file opens in the system preview,
 * moves up or goes away; new ones upload from the photo library or the files app onto the row page.
 */
import { uploadPageFile } from '@orbit/api/generated/sdk.gen'
import { MAX_FILES, rowFileUrl } from '@orbit/database/fields'
import type { CellValue, FileRef, Row } from '@orbit/database/types'
import { useRef, useState } from 'react'
import { Pressable, View } from 'react-native'

import { multipart } from '../../data/account-api'
import { useOrbit } from '../../orbit-context'
import { useTheme } from '../../theme'
import { Button, Icon, Text } from '../../ui'
import { type FileSource, openFile, pickFiles } from '../tasks/files'
import { SheetNote } from '../tasks/Sheet'

export function FilesEditor({ row, value, onCommit }: { row: Row; value: CellValue | null; onCommit: (value: CellValue | null) => void }) {
  const { client, session, workspaceId } = useOrbit()
  const { colors } = useTheme()
  const [files, setFiles] = useState<FileRef[]>(value?.kind === 'files' ? value.files : [])
  const latest = useRef(files)
  const [uploading, setUploading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const commit = (next: FileRef[]) => {
    latest.current = next
    setFiles(next)
    onCommit(next.length > 0 ? { kind: 'files', files: next } : null)
  }
  const add = async (source: FileSource) => {
    setError(null)
    const picked = await pickFiles(source)
    const room = MAX_FILES - latest.current.length
    if (picked.length > room) setError(`A value holds at most ${MAX_FILES} files.`)
    const batch = picked.slice(0, Math.max(room, 0))
    if (batch.length === 0) return
    setUploading(true)
    const added: FileRef[] = []
    for (const file of batch) {
      try {
        const { data } = await uploadPageFile({
          client,
          path: { workspace_id: workspaceId, page_id: row.id },
          body: { file: file.blob },
          bodySerializer: multipart(file.blob),
          throwOnError: true,
        })
        added.push({ file_id: data.id, name: data.file_name })
      } catch {
        setError(`${file.name} could not be uploaded.`)
      }
    }
    setUploading(false)
    if (added.length > 0) commit([...latest.current, ...added].slice(0, MAX_FILES))
  }
  const moveUp = (index: number) => {
    const next = [...files]
    const [file] = next.splice(index, 1)
    next.splice(index - 1, 0, file!)
    commit(next)
  }

  return (
    <View className="pb-2">
      {files.length === 0 ? <SheetNote>No files yet.</SheetNote> : null}
      {files.map((file, index) => (
        <View key={file.file_id} className="min-h-11 flex-row items-center gap-3 px-4">
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={`Open ${file.name}`}
            className="flex-1 flex-row items-center gap-2 min-w-0 active:opacity-60"
            onPress={() => void openFile(`${session.server}${rowFileUrl(workspaceId, row.id, file.file_id)}`, file.name, session.token).catch(() => setError(`${file.name} could not be opened.`))}
          >
            <Icon name="attach" size={16} color={colors.mutedForeground} />
            <Text numberOfLines={1} className="shrink text-[15px] text-foreground">
              {file.name}
            </Text>
          </Pressable>
          {index > 0 ? (
            <Pressable accessibilityRole="button" accessibilityLabel={`Move ${file.name} up`} hitSlop={8} onPress={() => moveUp(index)}>
              <Icon name="arrowUp" size={16} color={colors.mutedForeground} />
            </Pressable>
          ) : null}
          <Pressable accessibilityRole="button" accessibilityLabel={`Remove ${file.name}`} hitSlop={8} onPress={() => commit(files.filter((other) => other.file_id !== file.file_id))}>
            <Icon name="close" size={16} color={colors.mutedForeground} />
          </Pressable>
        </View>
      ))}
      {error ? <SheetNote tone="destructive">{error}</SheetNote> : null}
      <View className="flex-row gap-2 px-4 pt-2">
        <Button label={uploading ? 'Uploading…' : 'Photo library'} variant="outline" disabled={uploading || files.length >= MAX_FILES} onPress={() => void add('photos')} />
        <Button label="Files" variant="outline" disabled={uploading || files.length >= MAX_FILES} onPress={() => void add('files')} />
      </View>
    </View>
  )
}
