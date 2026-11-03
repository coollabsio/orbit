import { useState } from 'react'
import { ActivityIndicator, Alert, Image, Pressable, View } from 'react-native'

import { fileSize } from '../../data/tasks'
import { useOrbit } from '../../orbit-context'
import { errorMessage } from '../../session-context'
import { useTheme } from '../../theme'
import { Icon, Text } from '../../ui'
import { openFile } from './files'

export interface ShownFile {
  key: string
  name: string
  /** The server path, `/api/v1/…/download`; none while the file uploads. */
  path?: string
  image: boolean
  size?: number
  /** An admin deleted the stored file: it shows as "Deleted file" and does not open. */
  deleted?: boolean
}

/**
 * Files of a comment or of the task: images as thumbnails, other files as chips. A tap downloads the file with the
 * session token and opens it in the system preview. `onRemove` adds a remove button (a file waiting in the composer).
 */
export function Attachments({ files, onRemove }: { files: readonly ShownFile[]; onRemove?: (key: string) => void }) {
  const { session } = useOrbit()
  const [opening, setOpening] = useState<string | null>(null)
  if (files.length === 0) return null
  const open = async (file: ShownFile) => {
    if (opening || !file.path) return
    setOpening(file.key)
    try {
      await openFile(`${session.server}${file.path}`, file.name, session.token)
    } catch (error) {
      Alert.alert(`Could not open “${file.name}”`, errorMessage(error))
    } finally {
      setOpening(null)
    }
  }
  const images = files.filter((file) => file.image && !file.deleted)
  const others = files.filter((file) => !file.image || file.deleted)
  return (
    <View className="gap-1.5 mt-1.5">
      {images.length > 0 ? (
        <View className="flex-row flex-wrap gap-1.5">
          {images.map((file) => (
            <Thumbnail key={file.key} file={file} opening={opening === file.key} onPress={() => void open(file)} onRemove={onRemove && (() => onRemove(file.key))} />
          ))}
        </View>
      ) : null}
      {others.map((file) => file.deleted ? (
        <DeletedChip key={file.key} name={file.name} />
      ) : (
        <FileChip key={file.key} file={file} opening={opening === file.key} onPress={() => void open(file)} onRemove={onRemove && (() => onRemove(file.key))} />
      ))}
    </View>
  )
}

function Thumbnail({ file, opening, onPress, onRemove }: { file: ShownFile; opening: boolean; onPress: () => void; onRemove?: () => void }) {
  const { colors } = useTheme()
  const { session } = useOrbit()
  return (
    <View>
      <Pressable accessibilityRole="imagebutton" accessibilityLabel={`Open ${file.name}`} onPress={onPress} className="size-24 rounded-md overflow-hidden border border-border bg-muted">
        {file.path ? (
          <Image source={{ uri: `${session.server}${file.path}`, headers: { Authorization: `Bearer ${session.token}` } }} resizeMode="cover" className="size-full" accessibilityIgnoresInvertColors />
        ) : null}
        {opening || !file.path ? <ActivityIndicator className="absolute inset-0" color={colors.foreground} /> : null}
      </Pressable>
      {onRemove ? <RemoveButton name={file.name} onPress={onRemove} /> : null}
    </View>
  )
}

function FileChip({ file, opening, onPress, onRemove }: { file: ShownFile; opening: boolean; onPress: () => void; onRemove?: () => void }) {
  const { colors } = useTheme()
  return (
    <View className="self-start max-w-full">
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Open ${file.name}`}
        onPress={onPress}
        className={`min-h-9 flex-row items-center gap-2 pl-2.5 rounded-md border border-border bg-card active:bg-muted ${onRemove ? 'pr-8' : 'pr-2.5'}`}
      >
        {opening || !file.path ? <ActivityIndicator size="small" color={colors.mutedForeground} /> : <Icon name="documentDownload" size={16} color={colors.mutedForeground} />}
        <Text numberOfLines={1} className="font-noto-medium text-[13px] shrink">
          {file.name}
        </Text>
        {file.size !== undefined ? <Text className="text-[12px] text-muted-foreground">{fileSize(file.size)}</Text> : null}
      </Pressable>
      {onRemove ? <RemoveButton name={file.name} onPress={onRemove} center /> : null}
    </View>
  )
}

function DeletedChip({ name }: { name: string }) {
  const { colors } = useTheme()
  return (
    <View accessibilityLabel={`Deleted file ${name}`} className="self-start max-w-full min-h-9 flex-row items-center gap-2 px-2.5 rounded-md border border-dashed border-border">
      <Icon name="documentDownload" size={16} color={colors.mutedForeground} />
      <Text className="font-noto-medium text-[13px] text-muted-foreground">Deleted file</Text>
      <Text numberOfLines={1} className="text-[12px] text-muted-foreground shrink">{name}</Text>
    </View>
  )
}

/** Sits over the corner of a thumbnail, or (`center`) at the end of a file chip; a sibling, never inside the open button. */
function RemoveButton({ name, onPress, center = false }: { name: string; onPress: () => void; center?: boolean }) {
  const { colors } = useTheme()
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`Remove ${name}`}
      hitSlop={8}
      onPress={onPress}
      className={`absolute size-5 rounded-full items-center justify-center bg-popover border border-border ${center ? 'top-2 right-2' : 'top-1 right-1'}`}
    >
      <Icon name="close" size={12} color={colors.foreground} />
    </Pressable>
  )
}
