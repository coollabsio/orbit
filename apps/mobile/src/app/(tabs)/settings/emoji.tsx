import { useState } from 'react'
import { Image, Pressable, View } from 'react-native'

import { ActionSheet, ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { Note, SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { BUILT_IN_SHORTCODES, type CustomEmojiRecord, EMOJI_LIMIT, EMOJI_PROBLEMS, emojiNameFromFile, emojiProblem, type PickedImage, serverProblem } from '../../../data/custom-emoji-api'
import { shortDate } from '../../../data/format'
import { useServerImage } from '../../../hooks/chat'
import { type ImageSource, pickCustomImage, useCustomEmojiList, useEmojiWrites, useStandardEmojiNames } from '../../../hooks/custom-emoji'
import { useOrbit } from '../../../orbit-context'
import { useMembers } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { Button, EmptyState, Input, Text } from '../../../ui'

/** The web's Emoji page: the workspace's custom emoji, add one from an image, delete one. Owners and administrators only (`chat.manage`). */
export default function EmojiScreen() {
  const { workspace } = useOrbit()
  const canManage = workspace.permissions.includes('chat.manage')
  const emoji = useCustomEmojiList(canManage)
  const standard = useStandardEmojiNames(canManage)
  const members = useMembers()
  const { create, remove } = useEmojiWrites()
  const image = useServerImage()
  const [file, setFile] = useState<PickedImage | null>(null)
  const [name, setName] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  const [choosing, setChoosing] = useState(false)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)

  if (!canManage) {
    return (
      <SettingsShell title="Settings" nav={WORKSPACE_NAV}>
        <SettingsCard title="Emoji">
          <Note>Only workspace owners and administrators can manage custom emoji.</Note>
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
        setName(emojiNameFromFile(picked.name))
      },
      (error: unknown) => setProblem(`The image could not be opened. ${errorMessage(error)}`),
    )

  const submit = () => {
    if (!file || create.isPending) return
    // a standard emoji keeps its name (`:joy:` becomes the character as it is typed)
    const taken = (candidate: string) => Boolean(emoji.data?.some((entry) => entry.name === candidate)) || BUILT_IN_SHORTCODES.has(candidate) || Boolean(standard?.has(candidate))
    const refused = emojiProblem(file, name, taken)
    setProblem(refused)
    if (refused) return
    create.mutate(
      { name, image: file },
      {
        onSuccess: () => {
          setFile(null)
          setName('')
        },
        onError: (error) => setProblem(serverProblem(error, 'The emoji could not be added.', EMOJI_PROBLEMS)),
      },
    )
  }

  const askDelete = (entry: CustomEmojiRecord) =>
    setConfirm({
      title: `Delete :${entry.name}:?`,
      description: 'Messages and reactions that use it show its name as text. You cannot undo this.',
      confirmLabel: 'Delete',
      danger: true,
      onConfirm: () => remove.mutate(entry),
    })

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={() => emoji.refetch()}>
      <SettingsCard title="Add emoji" description="Custom emoji are for everyone in this workspace: in chat messages and as reactions.">
        <View className="gap-4">
          <ImageField file={file} description="PNG, JPEG, WebP or GIF, at most 256 KB." onPress={() => setChoosing(true)} />
          <Input
            label="Name"
            required
            value={name}
            maxLength={32}
            placeholder="party_parrot"
            autoCapitalize="none"
            autoCorrect={false}
            onChangeText={(value) => {
              setName(value.toLowerCase())
              setProblem(null)
            }}
            description={`Lower case letters, digits and underscores. In a message, type :${name || 'name'}:`}
          />
          <View className="flex-row flex-wrap items-center gap-3">
            <Button label="Add emoji" disabled={!file || !name || create.isPending} onPress={submit} />
            {problem ? (
              <Text accessibilityRole="alert" className="text-xs text-destructive">
                {problem}
              </Text>
            ) : null}
          </View>
        </View>
      </SettingsCard>
      {emoji.data?.length === 0 ? (
        // the web EmptyState is `min-h-80`
        <View className="min-h-80"><EmptyState icon="smile" title="No custom emoji yet" description="Add an image above. Everyone in the workspace can then use it in chat." /></View>
      ) : (
        <SettingsCard title="Emoji" description={emoji.data ? `${emoji.data.length} of ${EMOJI_LIMIT} in this workspace.` : undefined} flush>
          {emoji.isPending ? <SettingsRow first><Text className="text-[13px]">Loading emoji…</Text></SettingsRow> : null}
          {emoji.isError ? <SettingsRow first><Text accessibilityRole="alert" className="text-[13px]">Emoji could not be loaded.</Text></SettingsRow> : null}
          {emoji.data?.map((entry, index) => (
            <SettingsRow key={entry.id} first={index === 0}>
              <View className="size-9 shrink-0 items-center justify-center rounded-lg bg-muted">
                <Image source={image(entry.url)} accessibilityIgnoresInvertColors className="size-6" resizeMode="contain" />
              </View>
              <View className="min-w-0 flex-1 gap-0.5">
                <Text numberOfLines={1} className="font-noto-medium text-[13px] text-foreground">
                  :{entry.name}:
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
                {serverProblem(remove.error, 'The emoji could not be deleted.', EMOJI_PROBLEMS)}
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
