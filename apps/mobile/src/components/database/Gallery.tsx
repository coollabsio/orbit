/**
 * A gallery view at phone size (spec P3 16.1): cards in 2 columns (1 for large cards), each with its preview (the row
 * page's cover, its body's first image or text, or a files property's first image), the title and the card properties.
 */
import { cardFace, galleryOf, type CardFace } from '@orbit/database/gallery'
import type { Database, Row, ViewConfig, ViewContext } from '@orbit/database/types'
import { filledProperties } from '@orbit/database/values'
import { router } from 'expo-router'
import type { ReactElement } from 'react'
import { FlatList, Image, Pressable, View } from 'react-native'

import { useOrbit } from '../../orbit-context'
import { Text } from '../../ui'
import { CellValue, RowTitle } from './cells'

export function Gallery({ database, config, rows, ctx, header, empty, refresh }: {
  database: Database
  config: ViewConfig
  /** The view's rows (filtered, searched and sorted). */
  rows: Row[]
  ctx: ViewContext
  header: ReactElement
  empty: ReactElement
  refresh: ReactElement
}) {
  const { workspaceId } = useOrbit()
  const gallery = galleryOf(config)
  const columns = gallery.size === 'large' ? 1 : 2
  const cardProperties = config.card_properties.flatMap((id) => database.properties.find((property) => property.id === id) ?? [])
  return (
    <FlatList
      key={columns}
      data={rows}
      numColumns={columns}
      keyExtractor={(row) => row.id}
      ListHeaderComponent={header}
      ListEmptyComponent={empty}
      keyboardShouldPersistTaps="handled"
      contentContainerClassName="pb-8"
      columnWrapperClassName={columns > 1 ? 'justify-between px-3' : undefined}
      refreshControl={refresh}
      renderItem={({ item }) => {
        const face = cardFace(item, gallery, database.properties, workspaceId)
        return (
        <View className={columns > 1 ? 'w-[48.5%] mt-2.5' : 'mx-3 mt-2.5'}>
          <Pressable
            accessibilityRole="link"
            accessibilityLabel={item.title || 'Untitled'}
            onPress={() => router.push(`/docs/${item.id}`)}
            className="overflow-hidden rounded-md border border-border bg-card active:bg-muted"
          >
            {face ? <Preview face={face} fitImage={gallery.fitImage} /> : null}
            <View className="gap-1.5 px-2.5 py-2">
              <RowTitle row={item} />
              {filledProperties(item, cardProperties).map((property) => (
                <CellValue key={property.id} property={property} row={item} ctx={ctx} chip />
              ))}
            </View>
          </Pressable>
        </View>
        )
      }}
    />
  )
}

function Preview({ face, fitImage }: { face: CardFace; fitImage: boolean }) {
  const { session } = useOrbit()
  return (
    <View className="aspect-[3/2] w-full overflow-hidden border-b border-border bg-muted">
      {face.kind === 'image' ? (
        <Image
          source={/^https?:/.test(face.url) ? { uri: face.url } : { uri: `${session.server}${face.url}`, headers: { Authorization: `Bearer ${session.token}` } }}
          resizeMode={fitImage ? 'contain' : 'cover'}
          className="size-full"
          accessibilityIgnoresInvertColors
        />
      ) : face.kind === 'text' ? (
        <Text numberOfLines={5} className="px-2.5 pt-2 text-[12px] text-muted-foreground">
          {face.text}
        </Text>
      ) : null}
    </View>
  )
}
