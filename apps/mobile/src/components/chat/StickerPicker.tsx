import { useMemo, useState } from 'react'
import { Modal, Platform, Pressable, SectionList, useWindowDimensions, View } from 'react-native'

import * as chat from '../../data/chat-api'
import type { CustomStickerRecord } from '../../data/custom-emoji-api'
import { useKlipySearch, useServerImage } from '../../hooks/chat'
import { useTheme } from '../../theme'
import { Button, Icon, SafeAreaView, Spinner, Text } from '../../ui'
import { TextInput } from '../../ui/scaled'
import { Segmented } from '../settings/Shell'
import { ChatImage } from './ChatImage'
import type { StickerPick } from './Composer'

type Tab = 'gifs' | 'stickers'

interface Cell {
  key: string
  label: string
  /** A KLIPY URL, or a server path that needs the session's token. */
  uri: string
  server: boolean
  pick: StickerPick
}

interface Row {
  key: string
  cells: Cell[]
}

interface Section {
  key: string
  title: string
  data: Row[]
}

const rowsOf = (cells: readonly Cell[], columns: number, prefix: string): Row[] =>
  Array.from({ length: Math.ceil(cells.length / columns) }, (_, index) => ({ key: `${prefix}-${index}`, cells: cells.slice(index * columns, index * columns + columns) }))

const GAP = 4
const PADDING = 8

/**
 * GIFs and stickers as a sheet, like the emoji picker. "GIFs" lists KLIPY (only when KLIPY is on); "Stickers" lists the
 * workspace's stickers, then KLIPY's. A search field filters both; without words KLIPY shows what is trending. A pick
 * sends at once.
 */
export function StickerPicker({
  visible,
  klipy,
  stickers,
  onPick,
  onClose,
}: {
  visible: boolean
  /** KLIPY is on: the GIFs tab and KLIPY's stickers. */
  klipy: boolean
  stickers: readonly CustomStickerRecord[]
  onPick: (pick: StickerPick) => void
  onClose: () => void
}) {
  const { colors, scheme } = useTheme()
  const image = useServerImage()
  const { width } = useWindowDimensions()
  const [chosen, setChosen] = useState<Tab>('gifs')
  const [query, setQuery] = useState('')
  const tab: Tab = klipy ? chosen : 'stickers'
  const gifs = useKlipySearch('gifs', query, visible && klipy && tab === 'gifs')
  const klipyStickers = useKlipySearch('stickers', query, visible && klipy && tab === 'stickers')
  const active = tab === 'gifs' ? gifs : klipyStickers

  const inner = Math.min(width, 520) - PADDING * 2
  const gifColumns = 2
  const stickerColumns = Math.max(3, Math.floor((inner + GAP) / (84 + GAP)))
  const columns = tab === 'gifs' ? gifColumns : stickerColumns
  const cellWidth = (inner - (columns - 1) * GAP) / columns
  const cellHeight = tab === 'gifs' ? Math.round(cellWidth * 0.7) : cellWidth

  const klipyItems = useMemo(() => (active.data?.pages ?? []).flatMap((page) => page.items), [active.data])
  const sections = useMemo<Section[]>(() => {
    const klipyCells = klipyItems
      // a page boundary can repeat an item: keys must stay unique
      .filter((item, index, all) => all.findIndex((other) => other.id === item.id) === index)
      .map((item): Cell => ({ key: `klipy-${item.id}`, label: item.title || (item.kind === 'gif' ? 'GIF' : 'Sticker'), uri: item.preview.url, server: false, pick: { kind: 'klipy', item } }))
    const words = query.trim()
    const klipyTitle = words ? 'From KLIPY' : 'Trending on KLIPY'
    if (tab === 'gifs') return klipyCells.length > 0 ? [{ key: 'gifs', title: klipyTitle, data: rowsOf(klipyCells, columns, 'gifs') }] : []
    const lower = words.toLowerCase()
    const own = stickers
      .filter((sticker) => sticker.name.toLowerCase().includes(lower))
      .map((sticker): Cell => ({ key: `own-${sticker.id}`, label: sticker.name, uri: sticker.url, server: true, pick: { kind: 'sticker', sticker } }))
    return [
      ...(own.length > 0 ? [{ key: 'own', title: 'Workspace', data: rowsOf(own, columns, 'own') }] : []),
      ...(klipy && klipyCells.length > 0 ? [{ key: 'klipy', title: klipyTitle, data: rowsOf(klipyCells, columns, 'klipy') }] : []),
    ]
  }, [klipyItems, stickers, query, tab, columns, klipy])

  const close = () => {
    setQuery('')
    onClose()
  }
  const pick = (cell: Cell) => {
    onPick(cell.pick)
    close()
  }
  const more = () => {
    if (active.hasNextPage && !active.isFetchingNextPage && !active.isError) void active.fetchNextPage()
  }

  const placeholder = tab === 'gifs' ? 'Search KLIPY' : klipy ? 'Search stickers and KLIPY' : 'Search stickers'
  // what KLIPY's part of the list says while it has no items
  const klipyState = !klipy ? null : active.isPending ? 'loading' : active.isError && klipyItems.length === 0 ? 'error' : null
  const empty = sections.length === 0 && klipyState === null

  return (
    <Modal visible={visible} animationType="slide" presentationStyle="pageSheet" onRequestClose={close}>
      <SafeAreaView edges={Platform.OS === 'ios' ? ['bottom', 'left', 'right'] : ['top', 'bottom', 'left', 'right']} className="flex-1 bg-popover">
        <View className="flex-row items-center gap-2 p-2">
          <View className="flex-1 h-9 flex-row items-center gap-2 px-2.5 rounded-md bg-secondary">
            <Icon name="search" size={14} color={colors.mutedForeground} />
            <TextInput
              value={query}
              onChangeText={setQuery}
              placeholder={placeholder}
              accessibilityLabel={placeholder}
              placeholderTextColor={colors.mutedForeground}
              selectionColor={colors.primary}
              keyboardAppearance={scheme}
              autoCapitalize="none"
              autoCorrect={false}
              returnKeyType="search"
              className="font-noto text-[16px] leading-[1.45] flex-1 py-0 text-foreground"
            />
          </View>
          <Button label="Close" variant="ghost" onPress={close} />
        </View>
        {klipy ? (
          <View className="px-2 pb-1">
            <Segmented
              label="GIFs or stickers"
              value={tab}
              options={[
                { value: 'gifs', label: 'GIFs' },
                { value: 'stickers', label: 'Stickers' },
              ]}
              onChange={setChosen}
            />
          </View>
        ) : null}
        <SectionList
          sections={sections}
          keyExtractor={(row) => row.key}
          keyboardShouldPersistTaps="handled"
          keyboardDismissMode="on-drag"
          stickySectionHeadersEnabled
          initialNumToRender={8}
          contentContainerClassName="px-2 pb-4"
          onEndReached={more}
          onEndReachedThreshold={0.6}
          renderSectionHeader={({ section }) => <Text className="font-noto-medium text-[12px] px-1 pt-3 pb-1.5 text-muted-foreground bg-popover">{section.title}</Text>}
          renderItem={({ item }) => (
            <View className="flex-row mb-1 gap-1">
              {item.cells.map((cell) => (
                <Pressable
                  key={cell.key}
                  accessibilityRole="button"
                  accessibilityLabel={`Send ${cell.label}`}
                  onPress={() => pick(cell)}
                  className={`items-center justify-center rounded-md overflow-hidden active:opacity-70 ${tab === 'gifs' ? 'bg-muted' : ''}`}
                  style={{ width: cellWidth, height: cellHeight }}
                >
                  <ChatImage source={cell.server ? image(cell.uri) : { uri: cell.uri }} label={cell.label} contain={tab === 'stickers'} style={tab === 'gifs' ? { width: cellWidth, height: cellHeight } : { width: cellWidth - 8, height: cellHeight - 8 }} />
                </Pressable>
              ))}
            </View>
          )}
          ListFooterComponent={
            klipyState === 'loading' || active.isFetchingNextPage ? (
              <View className="p-4 items-center">
                <Spinner />
              </View>
            ) : klipyState === 'error' ? (
              <View className="p-4 items-center gap-2">
                <Text className="text-[13px] text-center text-muted-foreground">{chat.klipyProblem(active.error)}</Text>
                <Button label="Try again" variant="outline" onPress={() => void active.refetch()} />
              </View>
            ) : active.isError && klipyItems.length > 0 ? (
              <View className="p-4 items-center">
                <Button label="Load more" variant="ghost" onPress={() => void active.fetchNextPage()} />
              </View>
            ) : undefined
          }
          ListEmptyComponent={
            empty ? (
              <Text className="text-[14px] p-4 text-center text-muted-foreground">
                {tab === 'gifs' ? 'No GIFs found.' : query.trim() ? 'No stickers found.' : 'This workspace has no stickers yet.'}
              </Text>
            ) : undefined
          }
        />
        {klipy && (tab === 'gifs' || klipyItems.length > 0) ? (
          <Text className="text-[11px] leading-[16px] py-1.5 text-center text-muted-foreground border-t border-border">Powered by KLIPY</Text>
        ) : null}
      </SafeAreaView>
    </Modal>
  )
}
