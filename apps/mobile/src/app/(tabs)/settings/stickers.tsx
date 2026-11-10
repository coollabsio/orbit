import { useState } from 'react'
import { Image, Pressable, View } from 'react-native'

import { ActionSheet, ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { Note, SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { type CustomStickerRecord, type PickedImage, serverProblem, STICKER_LIMIT, STICKER_NAME_MAX, STICKER_PROBLEMS, stickerNameFromFile, stickerProblem } from '../../../data/custom-emoji-api'
import { shortDate } from '../../../data/format'
import { useServerImage } from '../../../hooks/chat'
import { type ImageSource, pickCustomImage, useCustomStickers, useStickerWrites } from '../../../hooks/custom-emoji'
import { useOrbit } from '../../../orbit-context'
import { useMembers } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { Button, EmptyState, Input, Text } from '../../../ui'

/** The web's Stickers page: the workspace's stickers, add one from an image, delete one. Owners and administrators only (`chat.manage`). */
export default function StickersScreen() {
  const { workspace } = useOrbit()
  const canManage = workspace.permissions.includes('chat.manage')
  const stickers = useCustomStickers(canManage)
  const members = useMembers()
  const { create, remove } = useStickerWrites()
  const image = useServerImage()
  const [file, setFile] = useState<PickedImage | null>(null)
  const [name, setName] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  const [choosing, setChoosing] = useState(false)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)

  if (!canManage) {
    return (
      <SettingsShell title="Settings" nav={WORKSPACE_NAV}>
        <SettingsCard title="Stickers">
          <Note>Only workspace owners and administrators can manage stickers.</Note>
        </SettingsCard>
      </SettingsShell>
    )
  }

  const memberNames = new Map((members.data ?? []).map((member) => [member.user_id, member.display_name]))

  const choose = (source: ImageSource) =>
    pickCustomImage(source).then(
      (picked) => {
        if (!picked) return
        setFile(picked)
        setProblem(null)
        setName(stickerNameFromFile(picked.name))
      },
      (error: unknown) => setProblem(`The image could not be opened. ${errorMessage(error)}`),
    )

  const submit = () => {
    if (!file || create.isPending) return
    const refused = stickerProblem(file, name, stickers.data?.map((entry) => entry.name) ?? [])
    setProblem(refused)
    if (refused) return
    create.mutate(
      { name: name.trim(), image: file },
      {
        onSuccess: () => {
          setFile(null)
          setName('')
        },
        onError: (error) => setProblem(serverProblem(error, 'The sticker could not be added.', STICKER_PROBLEMS)),
      },
    )
  }

  const askDelete = (entry: CustomStickerRecord) =>
    setConfirm({
      title: `Delete ${entry.name}?`,
      description: 'Messages that were sent with it say that the sticker was deleted. You cannot undo this.',
      confirmLabel: 'Delete',
      danger: true,
      onConfirm: () => remove.mutate(entry),
    })

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={() => stickers.refetch()}>
      <SettingsCard title="Add sticker" description="Stickers are for everyone in this workspace: a large image that is sent as a chat message.">
        <View className="gap-4">
          <ImageField file={file} description="PNG, JPEG, WebP or GIF, at most 512 KB." onPress={() => setChoosing(true)} />
          <Input
            label="Name"
            required
            value={name}
            maxLength={STICKER_NAME_MAX}
            placeholder="Party Parrot"
            onChangeText={(value) => {
              setName(value)
              setProblem(null)
            }}
            description="2 to 30 characters. People find the sticker by this name."
          />
          <View className="flex-row flex-wrap items-center gap-3">
            <Button label="Add sticker" disabled={!file || !name.trim() || create.isPending} onPress={submit} />
            {problem ? (
              <Text accessibilityRole="alert" className="text-xs text-destructive">
                {problem}
              </Text>
            ) : null}
          </View>
        </View>
      </SettingsCard>
      {stickers.data?.length === 0 ? (
        // the web EmptyState is `min-h-80`
        <View className="min-h-80"><EmptyState icon="sticker" title="No stickers yet" description="Add an image above. Everyone in the workspace can then send it in chat." /></View>
      ) : (
        <SettingsCard title="Stickers" description={stickers.data ? `${stickers.data.length} of ${STICKER_LIMIT} in this workspace.` : undefined} flush>
          {stickers.isPending ? <SettingsRow first><Note loading>Loading stickers…</Note></SettingsRow> : null}
          {stickers.isError ? <SettingsRow first><Text accessibilityRole="alert" className="text-[13px]">Stickers could not be loaded.</Text></SettingsRow> : null}
          {stickers.data?.map((entry, index) => (
            <SettingsRow key={entry.id} first={index === 0}>
              <Image source={image(entry.url)} accessibilityIgnoresInvertColors className="size-16 shrink-0" resizeMode="contain" />
              <View className="min-w-0 flex-1 gap-0.5">
                <Text numberOfLines={1} className="font-noto-medium text-[13px] text-foreground">
                  {entry.name}
                </Text>
                <Text numberOfLines={1} className="text-xs text-muted-foreground/70">
                  Added by {memberNames.get(entry.created_by) ?? 'a former member'} · {shortDate(entry.created_at)}
                </Text>
              </View>
              <Button label="Delete" variant="ghost" disabled={remove.isPending} onPress={() => askDelete(entry)} />
            </SettingsRow>
          ))}
          {remove.isError ? (
            <SettingsRow>
              <Text accessibilityRole="alert" className="text-xs text-destructive">
                {serverProblem(remove.error, 'The sticker could not be deleted.', STICKER_PROBLEMS)}
              </Text>
            </SettingsRow>
          ) : null}
        </SettingsCard>
      )}
      <ActionSheet
        visible={choosing}
        title="Image"
        actions={[
          { label: 'Photo library', icon: 'sticker', onPress: () => void choose('photos') },
          { label: 'Choose file', icon: 'attach', onPress: () => void choose('files') },
        ]}
        onClose={() => setChoosing(false)}
      />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsShell>
  )
}

/** The web's file `Input`: a field with "Choose File" and the chosen name, the label above and the help below. */
function ImageField({ file, description, onPress }: { file: PickedImage | null; description: string; onPress: () => void }) {
  return (
    <View className="gap-2">
      <Text className="font-noto-medium text-[14px] leading-[19.25px]">
        Image
        <Text className="font-noto-semibold text-[14px] leading-[19.25px] text-primary">{'  *'}</Text>
      </Text>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={file ? `Image: ${file.name}. Choose another` : 'Choose an image'}
        onPress={onPress}
        className="min-h-8 flex-row items-center gap-1 rounded-lg border border-input px-2.5 py-[3px] dark:bg-input/30 active:bg-input/50"
      >
        <Text className="font-noto-medium text-[14px] text-foreground">Choose File</Text>
        <Text numberOfLines={1} className="min-w-0 shrink text-[16px] leading-[24px] text-foreground">
          {file ? file.name : 'No file chosen'}
        </Text>
      </Pressable>
      <Text className="text-[14px] leading-[21px] text-muted-foreground">{description}</Text>
    </View>
  )
}
