/**
 * The web chat dialogs as phone sheets: New channel, Edit channel, New or Rename category, New message and Add people.
 * Each is mounted while it is open, so it starts from the saved values every time.
 */
import { ApiProblem } from '@orbit/api/problem'
import { router } from 'expo-router'
import { useState } from 'react'
import { Pressable, Switch, View } from 'react-native'

import { CHANNEL_NAME_MAX, channelNameFinal, channelNameInput, matchPeople, messageablePeople, type Person, pickablePeople } from '../../data/chat'
import * as chat from '../../data/chat-api'
import type { CategoryRecord, ConversationRecord } from '../../data/orbit'
import { useChatWrite } from '../../hooks/chat'
import { useOrbit } from '../../orbit-context'
import { useChatCategories, useMembers } from '../../queries'
import { useTheme } from '../../theme'
import { Avatar, Icon, Input, PickerSheet, Text } from '../../ui'
import { ChoiceGroup, FormSheet } from './Sheet'

const isConflict = (error: unknown) => error instanceof ApiProblem && error.status === 409

/** Category field: a row that opens a picker of "No category" and the workspace's categories. */
function CategoryField({ value, onChange }: { value: string | null; onChange: (id: string | null) => void }) {
  const { colors } = useTheme()
  const categories = useChatCategories().data ?? []
  const [open, setOpen] = useState(false)
  const current = categories.find((category) => category.id === value)
  return (
    <View className="gap-2">
      <Text className="font-noto-medium text-[14px] leading-[19.25px]">Category</Text>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Category, ${current?.name ?? 'No category'}`}
        onPress={() => setOpen(true)}
        className="h-9 flex-row items-center px-2.5 rounded-lg border border-input"
      >
        <Text className="text-[15px] flex-1">{current?.name ?? 'No category'}</Text>
        <Icon name="chevronDown" size={16} color={colors.mutedForeground} />
      </Pressable>
      <PickerSheet
        visible={open}
        title="Category"
        options={[{ key: '', label: 'No category', selected: !current }, ...categories.map((category) => ({ key: category.id, label: category.name, selected: category.id === current?.id }))]}
        onSelect={(key) => {
          onChange(key || null)
          setOpen(false)
        }}
        onClose={() => setOpen(false)}
      />
    </View>
  )
}

/** The `#` field of a channel name: lowercase, hyphens for spaces. */
function NameField({ value, onChange, error }: { value: string; onChange: (value: string) => void; error?: string }) {
  return (
    <Input
      label="Name"
      required
      autoFocus
      autoCapitalize="none"
      autoCorrect={false}
      maxLength={CHANNEL_NAME_MAX}
      placeholder="design-team"
      value={value}
      onChangeText={(text) => onChange(channelNameInput(text))}
      description="Lowercase, with hyphens in place of spaces."
      error={error}
    />
  )
}

export function NewChannelSheet({ categoryId, onClose }: { categoryId: string | null; onClose: () => void }) {
  const { user } = useOrbit()
  const members = useMembers().data ?? []
  const run = useChatWrite()
  const [name, setName] = useState('')
  const [topic, setTopic] = useState('')
  const [category, setCategory] = useState(categoryId)
  const [kind, setKind] = useState<'public' | 'private'>('public')
  const [memberIds, setMemberIds] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const [taken, setTaken] = useState(false)
  const finalName = channelNameFinal(name)

  const submit = async () => {
    setBusy(true)
    const channel = await run(
      'The channel was not created',
      (client, workspaceId) => chat.createChannel(client, workspaceId, { name: finalName, topic: topic.trim(), categoryId: category, kind, memberIds: kind === 'private' ? memberIds : [] }),
      (error) => isConflict(error) && (setTaken(true), true),
    )
    setBusy(false)
    if (channel) {
      onClose()
      router.push(`/chat/${channel.id}`)
    }
  }

  return (
    <FormSheet visible title="New channel" description="Channels keep a conversation about one topic in one place." submitLabel="Create" canSubmit={finalName !== ''} busy={busy} onSubmit={() => void submit()} onClose={onClose}>
      <NameField
        value={name}
        onChange={(value) => {
          setName(value)
          setTaken(false)
        }}
        error={taken ? 'A channel with this name already exists.' : undefined}
      />
      <Input label="Topic" value={topic} maxLength={250} placeholder="What this channel is about" onChangeText={setTopic} />
      <CategoryField value={category} onChange={setCategory} />
      <ChoiceGroup
        label="Visibility"
        value={kind}
        onChange={setKind}
        options={[
          { value: 'public', label: 'Public', description: 'Everyone in the workspace is in it.', icon: 'hashtag' },
          { value: 'private', label: 'Private', description: 'Only the people you add can see it.', icon: 'lock' },
        ]}
      />
      {kind === 'private' ? (
        <View className="gap-2">
          <Text className="font-noto-medium text-[14px] leading-[19.25px]">Add people</Text>
          <MemberPicker people={pickablePeople(members, user.id)} selectedIds={memberIds} onChange={setMemberIds} />
          <Text className="text-[14px] leading-[21px] text-muted-foreground">You are a member already. You can add more people later.</Text>
        </View>
      ) : null}
    </FormSheet>
  )
}

export function EditChannelSheet({ conversation, onClose }: { conversation: ConversationRecord; onClose: () => void }) {
  const { colors } = useTheme()
  const run = useChatWrite()
  const [name, setName] = useState(conversation.name)
  const [topic, setTopic] = useState(conversation.topic)
  const [category, setCategory] = useState(conversation.category_id)
  const [isPrivate, setIsPrivate] = useState(conversation.kind === 'private')
  const [busy, setBusy] = useState(false)
  const [taken, setTaken] = useState(false)
  const finalName = channelNameFinal(name)

  const submit = async () => {
    // only what changed is sent, so a save does not undo somebody else's change to another field
    const patch: { name?: string; topic?: string; category_id?: string | null; kind?: 'public' | 'private' } = {}
    if (finalName !== conversation.name) patch.name = finalName
    if (topic.trim() !== conversation.topic) patch.topic = topic.trim()
    if (category !== conversation.category_id) patch.category_id = category
    const kind = isPrivate ? 'private' : 'public'
    if (kind !== conversation.kind) patch.kind = kind
    if (Object.keys(patch).length === 0) return onClose()
    setBusy(true)
    const saved = await run(
      'The changes were not saved',
      (client, workspaceId) => chat.updateChannel(client, workspaceId, conversation.id, patch),
      (error) => isConflict(error) && (setTaken(true), true),
    )
    setBusy(false)
    if (saved) onClose()
  }

  return (
    <FormSheet visible title="Edit channel" submitLabel="Save" canSubmit={finalName !== ''} busy={busy} onSubmit={() => void submit()} onClose={onClose}>
      <NameField
        value={name}
        onChange={(value) => {
          setName(value)
          setTaken(false)
        }}
        error={taken ? 'A channel with this name already exists.' : undefined}
      />
      <Input label="Topic" value={topic} maxLength={250} placeholder="What this channel is about" onChangeText={setTopic} />
      <CategoryField value={category} onChange={setCategory} />
      <View className={`flex-row items-center gap-3 ${conversation.is_default ? 'opacity-50' : 'opacity-100'}`}>
        <View className="flex-1 gap-1">
          <Text className="font-noto-medium text-[14px] leading-[19.25px]">Private channel</Text>
          <Text className="text-[13px] leading-[18px] text-muted-foreground">
            {conversation.is_default
              ? `#${conversation.name} is the default channel. Everybody is in it, so it cannot be private.`
              : isPrivate
                ? 'Only the people in it can see it. New people must be added.'
                : 'Everyone in the workspace is in it.'}
          </Text>
        </View>
        <Switch accessibilityLabel="Private channel" value={isPrivate} disabled={conversation.is_default} onValueChange={setIsPrivate} trackColor={{ true: colors.primary, false: colors.input }} />
      </View>
    </FormSheet>
  )
}

/** Creates a shared category, or renames `category`. */
export function CategorySheet({ category, onClose }: { category?: CategoryRecord; onClose: () => void }) {
  const run = useChatWrite()
  const [name, setName] = useState(category?.name ?? '')
  const [busy, setBusy] = useState(false)
  const trimmed = name.trim()
  const submit = async () => {
    if (category && trimmed === category.name) return onClose()
    setBusy(true)
    const done = await run(category ? 'The category was not renamed' : 'The category was not created', (client, workspaceId) =>
      category ? chat.renameCategory(client, workspaceId, category.id, trimmed) : chat.createCategory(client, workspaceId, trimmed),
    )
    setBusy(false)
    if (done) onClose()
  }
  return (
    <FormSheet visible title={category ? 'Rename category' : 'New category'} submitLabel={category ? 'Rename' : 'Create'} canSubmit={trimmed !== ''} busy={busy} onSubmit={() => void submit()} onClose={onClose}>
      <Input label="Name" required autoFocus maxLength={60} placeholder="Projects" value={name} onChangeText={setName} />
    </FormSheet>
  )
}

/** Picks people and opens the direct message with exactly them; only yourself opens your notes. */
export function NewMessageSheet({ onClose }: { onClose: () => void }) {
  const { user } = useOrbit()
  const members = useMembers().data ?? []
  const run = useChatWrite()
  const [selectedIds, setSelectedIds] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    setBusy(true)
    // the server adds the caller to every DM: only yourself is the DM with yourself
    const dm = await run('The conversation was not opened', (client, workspaceId) => chat.openDm(client, workspaceId, selectedIds.filter((id) => id !== user.id)))
    setBusy(false)
    if (dm) {
      onClose()
      router.push(`/chat/${dm.id}`)
    }
  }
  return (
    <FormSheet visible title="New message" description="Choose one person, several for a group conversation, or yourself for notes." submitLabel="Open" canSubmit={selectedIds.length > 0} busy={busy} onSubmit={() => void submit()} onClose={onClose}>
      <MemberPicker autoFocus people={messageablePeople(members, user.id)} selectedIds={selectedIds} onChange={setSelectedIds} />
    </FormSheet>
  )
}

/** Adds workspace members who are not in the channel yet. */
export function AddPeopleSheet({ conversation, onClose }: { conversation: ConversationRecord; onClose: () => void }) {
  const { user } = useOrbit()
  const members = useMembers().data ?? []
  const run = useChatWrite()
  const [selectedIds, setSelectedIds] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    setBusy(true)
    const done = await run('Could not add people', (client, workspaceId) => chat.addMembers(client, workspaceId, conversation.id, selectedIds))
    setBusy(false)
    if (done) onClose()
  }
  return (
    <FormSheet
      visible
      title={`Add people to #${conversation.name}`}
      submitLabel={selectedIds.length > 1 ? `Add ${selectedIds.length}` : 'Add'}
      canSubmit={selectedIds.length > 0}
      busy={busy}
      onSubmit={() => void submit()}
      onClose={onClose}
    >
      <MemberPicker autoFocus people={pickablePeople(members, user.id, conversation.member_ids)} selectedIds={selectedIds} onChange={setSelectedIds} />
    </FormSheet>
  )
}

/** The web `MemberPicker`: the chosen people as chips, a search field, and the list with a check on each chosen one. */
export function MemberPicker({ people, selectedIds, onChange, autoFocus = false }: { people: readonly Person[]; selectedIds: readonly string[]; onChange: (ids: string[]) => void; autoFocus?: boolean }) {
  const { colors } = useTheme()
  const [query, setQuery] = useState('')
  const toggle = (id: string) => onChange(selectedIds.includes(id) ? selectedIds.filter((other) => other !== id) : [...selectedIds, id])
  const chosen = selectedIds.flatMap((id) => people.find((person) => person.id === id) ?? [])
  const shown = matchPeople(people, query)
  return (
    <View className="gap-2">
      {chosen.length > 0 ? (
        <View className="flex-row flex-wrap gap-1.5">
          {chosen.map((person) => (
            <Pressable
              key={person.id}
              accessibilityRole="button"
              accessibilityLabel={`Remove ${person.name}`}
              onPress={() => toggle(person.id)}
              className="h-7 flex-row items-center gap-1.5 pl-1 pr-2 rounded-full bg-secondary"
            >
              <Avatar id={person.id} name={person.name} size={20} />
              <Text className="font-noto-medium text-[13px] text-secondary-foreground">{person.name}</Text>
              <Icon name="x" size={12} color={colors.mutedForeground} />
            </Pressable>
          ))}
        </View>
      ) : null}
      <Input autoFocus={autoFocus} placeholder="Search people" value={query} onChangeText={setQuery} autoCapitalize="none" autoCorrect={false} accessibilityLabel="Search people" />
      <View>
        {shown.length === 0 ? <Text className="text-[14px] p-2 text-muted-foreground">No people found.</Text> : null}
        {shown.map((person) => {
          const selected = selectedIds.includes(person.id)
          return (
            <Pressable
              key={person.id}
              accessibilityRole="checkbox"
              accessibilityState={{ checked: selected }}
              accessibilityLabel={person.name}
              onPress={() => toggle(person.id)}
              className="h-11 flex-row items-center gap-2.5 px-2 rounded-md active:bg-muted/80"
            >
              <Avatar id={person.id} name={person.name} size={28} />
              <Text numberOfLines={1} className="text-[14px] flex-1">
                {person.name}
              </Text>
              <View
                className={`size-4.5 rounded-[5px] border items-center justify-center ${selected ? 'border-primary bg-primary' : 'border-input'}`}
              >
                {selected ? <Icon name="check" size={12} color={colors.primaryForeground} /> : null}
              </View>
            </Pressable>
          )
        })}
      </View>
    </View>
  )
}
