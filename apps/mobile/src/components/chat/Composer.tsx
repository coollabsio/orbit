import { useQueryClient } from '@tanstack/react-query'
import { applyChatActions, applyChatEvents, chatQueryKeys } from '../../chat-cache'
import * as DocumentPicker from 'expo-document-picker'
import * as ImagePicker from 'expo-image-picker'
import { useState } from 'react'
import { Image, Platform, Pressable, ScrollView, View } from 'react-native'
import { TextInput } from '../../ui/scaled'

import { newNonce, type ThreadPages, upsertThreadMessage } from '../../data/chat'
import * as chat from '../../data/chat-api'
import type { MessageRecord } from '../../data/orbit'
import { showError } from '../../hooks/chat'

import { useOrbit } from '../../orbit-context'
import { useTheme } from '../../theme'
import { Icon, IconButton, Spinner, Text } from '../../ui'
import { uploadPart } from '../../upload-part'
import { EmojiPicker } from './EmojiPicker'
import { ActionSheet } from './Sheet'

/** A picked file: it uploads at once, and the message names it once it is there. */
interface DraftFile {
  key: string
  name: string
  /** Local preview of an image. */
  previewUri: string | null
  status: 'uploading' | 'ready' | 'failed'
  file: chat.ChatFileRecord | null
}

/** A message the server has not confirmed yet. */
export interface PendingMessage {
  nonce: string
  body: string
  files: number
}

/** What the composer writes: a new message (maybe quoting one) or an edit of your own. */
type Mode = { kind: 'new'; replyTo: MessageRecord | null } | { kind: 'edit'; message: MessageRecord }

/**
 * The composer's state and its writes for one conversation or thread: the draft, the picked files and their uploads,
 * the reply or edit target, and the messages on their way.
 */
export function useComposer(conversationId: string, threadRootId: string | null) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState('')
  const [files, setFiles] = useState<DraftFile[]>([])
  const [mode, setMode] = useState<Mode>({ kind: 'new', replyTo: null })
  const [alsoInChannel, setAlsoInChannel] = useState(false)
  const [pending, setPending] = useState<PendingMessage[]>([])

  const upload = (key: string, picked: { uri: string; name: string; width?: number; height?: number }) =>
    uploadPart(picked.uri, picked.name)
      .then((part) => chat.uploadFile(client, workspaceId, { part, width: picked.width, height: picked.height }))
      .then(
      (file) => setFiles((list) => list.map((item) => (item.key === key ? { ...item, status: 'ready', file } : item))),
      (error) => {
        setFiles((list) => list.map((item) => (item.key === key ? { ...item, status: 'failed' } : item)))
        showError('The file was not uploaded', error)
      },
    )

  const addFiles = (picked: { name: string; uri: string; image: boolean; width?: number; height?: number }[]) => {
    for (const item of picked) {
      const key = newNonce()
      setFiles((list) => [...list, { key, name: item.name, previewUri: item.image ? item.uri : null, status: 'uploading', file: null }])
      void upload(key, item)
    }
  }

  const received = (message: MessageRecord) => {
    applyChatActions(queryClient, workspaceId, [{ kind: 'upsert-message', conversationId, message }])
    if (message.thread_root_id) {
      queryClient.setQueryData<ThreadPages>(chatQueryKeys.thread(workspaceId, message.thread_root_id), (data) => data && upsertThreadMessage(data, message))
    }
  }

  const ready = files.filter((item) => item.status === 'ready')
  const uploading = files.some((item) => item.status === 'uploading')
  const canSend = !uploading && (draft.trim() !== '' || (mode.kind === 'new' && ready.length > 0))

  const send = () => {
    const body = draft.trim()
    if (!canSend) return
    if (mode.kind === 'edit') {
      const { message } = mode
      setDraft('')
      setMode({ kind: 'new', replyTo: null })
      if (body === message.body) return
      chat.editMessage(client, workspaceId, message.id, body).then(
        ({ result, events }) => {
          applyChatEvents(queryClient, workspaceId, events)
          received(result)
        },
        (error) => {
          setDraft(body)
          setMode({ kind: 'edit', message })
          showError('The message was not edited', error)
        },
      )
      return
    }
    const item = { nonce: newNonce(), body, files: ready.length }
    const replyTo = mode.replyTo
    const fileIds = ready.flatMap((file) => (file.file ? [file.file.id] : []))
    setDraft('')
    setFiles([])
    setMode({ kind: 'new', replyTo: null })
    setPending((list) => [...list, item])
    chat.send(client, workspaceId, conversationId, { body, nonce: item.nonce, threadRootId, replyToId: replyTo?.id ?? null, fileIds, alsoInChannel: threadRootId !== null && alsoInChannel }).then(
      ({ result, events }) => {
        applyChatEvents(queryClient, workspaceId, events)
        received(result)
        setPending((list) => list.filter((other) => other.nonce !== item.nonce))
      },
      (error) => {
        setPending((list) => list.filter((other) => other.nonce !== item.nonce))
        setDraft((current) => current || body)
        showError('Message not sent', error)
      },
    )
  }

  return {
    draft,
    setDraft,
    files,
    removeFile: (key: string) => setFiles((list) => list.filter((item) => item.key !== key)),
    addFiles,
    mode,
    reply: (message: MessageRecord) => setMode({ kind: 'new', replyTo: message }),
    edit: (message: MessageRecord) => {
      setMode({ kind: 'edit', message })
      setDraft(message.body)
    },
    cancelMode: () => {
      if (mode.kind === 'edit') setDraft('')
      setMode({ kind: 'new', replyTo: null })
    },
    alsoInChannel,
    setAlsoInChannel,
    pending,
    canSend,
    send,
  }
}

export type ComposerState = ReturnType<typeof useComposer>

/**
 * The web chat composer: the reply or edit banner, the picked files, the field, then attach / emoji / mention on the
 * left and send on the right. In a thread, "Also send to" the conversation sits under it.
 */
export function Composer({ state, placeholder, nameOf, alsoSendTo }: { state: ComposerState; placeholder: string; nameOf: (userId: string) => string; alsoSendTo?: string }) {
  const { colors, scheme } = useTheme()
  const [focused, setFocused] = useState(false)
  const [attachOpen, setAttachOpen] = useState(false)
  const [emojiOpen, setEmojiOpen] = useState(false)
  const { draft, setDraft, mode } = state
  const append = (text: string) => setDraft(`${draft}${draft && !draft.endsWith(' ') ? ' ' : ''}${text}`)

  const pickImages = async (camera: boolean) => {
    const options: ImagePicker.ImagePickerOptions = { mediaTypes: ['images', 'videos'], allowsMultipleSelection: !camera, selectionLimit: 10, quality: 0.9 }
    if (camera) {
      const permission = await ImagePicker.requestCameraPermissionsAsync()
      if (!permission.granted) return showError('No camera access', new Error('Allow Orbit to use the camera in Settings.'))
    }
    const result = camera ? await ImagePicker.launchCameraAsync(options) : await ImagePicker.launchImageLibraryAsync(options)
    if (result.canceled) return
    state.addFiles(
      result.assets.map((asset) => ({
        name: asset.fileName ?? `photo-${Date.now()}.${asset.mimeType?.split('/')[1] ?? 'jpg'}`,
        uri: asset.uri,
        image: asset.type === 'image',
        width: asset.width || undefined,
        height: asset.height || undefined,
      })),
    )
  }
  const pickDocuments = async () => {
    const result = await DocumentPicker.getDocumentAsync({ multiple: true, copyToCacheDirectory: true })
    if (result.canceled) return
    state.addFiles(
      result.assets.map((asset) => ({
        name: asset.name,
        uri: asset.uri,
        image: asset.mimeType?.startsWith('image/') ?? false,
      })),
    )
  }

  const banner = mode.kind === 'edit' ? 'Editing message' : mode.replyTo ? `Replying to ${nameOf(mode.replyTo.author_id)}` : null
  return (
    <View className="m-2 gap-1.5">
      {banner ? (
        <View className="flex-row items-center gap-1.5 pl-2.5">
          <Icon name={mode.kind === 'edit' ? 'edit' : 'reply'} size={14} color={colors.mutedForeground} />
          <Text numberOfLines={1} className="text-[12px] flex-1 text-muted-foreground">
            {banner}
          </Text>
          <IconButton icon="x" label={mode.kind === 'edit' ? 'Cancel editing' : 'Cancel reply'} size={24} iconSize={14} color={colors.mutedForeground} onPress={state.cancelMode} />
        </View>
      ) : null}
      <View className={`rounded-lg border bg-input/30 ${focused ? 'border-ring' : 'border-input'}`}>
        {state.files.length > 0 ? (
          <ScrollView horizontal keyboardShouldPersistTaps="handled" contentContainerClassName="gap-1.5 p-2 pb-0">
            {state.files.map((file) => (
              <View key={file.key} className={`h-14 flex-row items-center gap-2 pr-7 rounded-md border bg-background overflow-hidden ${file.previewUri ? 'pl-0' : 'pl-2.5'} ${file.status === 'failed' ? 'border-destructive' : 'border-border'}`}>
                {file.previewUri ? <Image source={{ uri: file.previewUri }} className="size-14" /> : <Icon name="attach" size={18} color={colors.mutedForeground} />}
                <View className="max-w-35">
                  <Text numberOfLines={1} className="font-noto-medium text-[12px]">
                    {file.name}
                  </Text>
                  <Text className={`text-[11px] ${file.status === 'failed' ? 'text-destructive' : 'text-muted-foreground'}`}>{file.status === 'uploading' ? 'Uploading…' : file.status === 'failed' ? 'Upload failed' : 'Ready'}</Text>
                </View>
                {file.status === 'uploading' ? <Spinner /> : null}
                <View className="absolute top-0.5 right-0.5">
                  <IconButton icon="x" label={`Remove ${file.name}`} size={22} iconSize={12} color={colors.mutedForeground} onPress={() => state.removeFile(file.key)} />
                </View>
              </View>
            ))}
          </ScrollView>
        ) : null}
        <TextInput
          value={draft}
          onChangeText={setDraft}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          placeholder={placeholder}
          placeholderTextColor={colors.mutedForeground}
          selectionColor={colors.primary}
          keyboardAppearance={scheme}
          multiline
          numberOfLines={1}
          accessibilityLabel={placeholder}
          className="min-h-10 max-h-40 px-3 py-2 text-[16px] leading-[24px] font-noto text-foreground"
        />
        <View className="flex-row items-center gap-1 p-1.5">
          <IconButton icon="attach" label="Attach files" size={32} iconSize={20} disabled={mode.kind === 'edit'} onPress={() => setAttachOpen(true)} />
          <IconButton icon="smile" label="Emoji" size={32} iconSize={20} onPress={() => setEmojiOpen(true)} />
          <IconButton icon="at" label="Mention" size={32} iconSize={20} onPress={() => append('@')} />
          <View className="flex-1" />
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={mode.kind === 'edit' ? 'Save' : 'Send'}
            accessibilityState={{ disabled: !state.canSend }}
            disabled={!state.canSend}
            onPress={state.send}
            hitSlop={4}
            className={`size-8 rounded-lg items-center justify-center active:bg-muted ${state.canSend ? 'opacity-100' : 'opacity-50'}`}
          >
            <Icon name={mode.kind === 'edit' ? 'check' : 'send'} size={20} color={colors.foreground} />
          </Pressable>
        </View>
      </View>
      {alsoSendTo !== undefined ? (
        <Pressable
          accessibilityRole="checkbox"
          accessibilityState={{ checked: state.alsoInChannel }}
          onPress={() => state.setAlsoInChannel(!state.alsoInChannel)}
          className="flex-row items-center gap-2 px-1 py-0.5"
        >
          <View className={`size-4 rounded-[4px] border items-center justify-center ${state.alsoInChannel ? 'border-primary bg-primary' : 'border-input'}`}>
            {state.alsoInChannel ? <Icon name="check" size={11} color={colors.primaryForeground} /> : null}
          </View>
          <Text className="text-[12px] text-muted-foreground">Also send to {alsoSendTo}</Text>
        </Pressable>
      ) : null}
      <ActionSheet
        visible={attachOpen}
        title="Attach"
        items={[
          { key: 'library', label: 'Photo library', icon: 'display', onPress: () => void pickImages(false).catch((error) => showError('Could not open the photo library', error)) },
          ...(Platform.OS === 'web' ? [] : [{ key: 'camera', label: 'Take a photo', icon: 'display' as const, onPress: () => void pickImages(true).catch((error) => showError('Could not open the camera', error)) }]),
          { key: 'file', label: 'Choose a file', icon: 'documentDownload', onPress: () => void pickDocuments().catch((error) => showError('Could not open the files', error)) },
        ]}
        onClose={() => setAttachOpen(false)}
      />
      <EmojiPicker visible={emojiOpen} onPick={(emoji) => setDraft(`${draft}${emoji}`)} onClose={() => setEmojiOpen(false)} />
    </View>
  )
}
