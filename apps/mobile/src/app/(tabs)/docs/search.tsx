import type { TextRange } from '@orbit/api/generated/types.gen'
import { router } from 'expo-router'
import { useState } from 'react'
import { FlatList, Pressable, View } from 'react-native'

import { splitByRanges } from '../../../data/chat'
import { pageTitle, searchHint } from '../../../data/docs'
import { usePageSearch, useRecentPages } from '../../../hooks/docs'
import { useTeamspaces } from '../../../queries'
import { alpha, useTheme } from '../../../theme'
import { EmptyState, ErrorView, Icon, IconButton, LoadingView, PaneHeader, SafeAreaView, Text } from '../../../ui'
import { TextInput } from '../../../ui/scaled'

interface Hit {
  id: string
  title: string
  icon: string | null
  hint: string
  titleHighlights?: TextRange[]
  snippet?: { text: string; highlights: TextRange[] }
}

/**
 * The web command palette's page search at phone size: the pages opened last while the field is empty, then the
 * server's title, body and property-value matches with their highlights. A database row says which database.
 */
export default function DocsSearchScreen() {
  const { colors, scheme } = useTheme()
  const [query, setQuery] = useState('')
  const search = usePageSearch(query)
  const recent = useRecentPages()
  const teamspaces = useTeamspaces()
  const searching = query.trim() !== ''
  const muted = alpha(colors.mutedForeground, 0.7)

  const hits: Hit[] = searching
    ? (search.data ?? []).map((page) => ({
        id: page.id,
        title: pageTitle(page),
        icon: page.icon,
        hint: searchHint(page, teamspaces.data),
        // the ranges index the page's own title; the "Untitled" fallback has none
        titleHighlights: page.title ? page.title_highlights : [],
        snippet: page.snippet ? { text: page.snippet, highlights: page.snippet_highlights } : undefined,
      }))
    : (recent.data ?? []).map((page) => ({ id: page.id, title: pageTitle(page), icon: page.icon, hint: searchHint({ ...page, database_title: null }, teamspaces.data) }))
  const active = searching ? search : recent

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <IconButton icon="back" label="Back to docs" color={muted} onPress={() => (router.canGoBack() ? router.back() : router.navigate('/docs'))} />
        <View className="flex-1 h-9 flex-row items-center gap-2 px-2.5 rounded-lg bg-secondary/60">
          <Icon name="search" size={16} color={colors.mutedForeground} />
          <TextInput
            autoFocus
            value={query}
            onChangeText={setQuery}
            returnKeyType="search"
            placeholder="Search pages and rows"
            accessibilityLabel="Search docs"
            placeholderTextColor={colors.mutedForeground}
            selectionColor={colors.primary}
            keyboardAppearance={scheme}
            autoCapitalize="none"
            autoCorrect={false}
            className="font-noto text-[16px] leading-[1.45] flex-1 py-0 text-foreground"
          />
        </View>
      </PaneHeader>
      {active.isPending && (searching || recent.fetchStatus !== 'idle') ? (
        <LoadingView />
      ) : active.isError && hits.length === 0 ? (
        <ErrorView error={active.error} onRetry={() => void active.refetch()} />
      ) : hits.length === 0 ? (
        <EmptyState icon="search" title={searching ? `No results for "${query.trim()}"` : 'Search docs'} description={searching ? 'Try other words.' : 'Find a page by its title, its text or a row by its property values.'} />
      ) : (
        <FlatList
          data={hits}
          keyExtractor={(hit) => hit.id}
          keyboardShouldPersistTaps="handled"
          contentContainerClassName="p-2 gap-px"
          ListHeaderComponent={searching ? undefined : <Text className="px-2 pb-1 font-noto-medium text-[12px] text-muted-foreground">Recent</Text>}
          renderItem={({ item: hit }) => (
            <Pressable accessibilityRole="link" accessibilityLabel={hit.title} onPress={() => router.push(`/docs/${hit.id}`)} className="flex-row items-center gap-2.5 px-2.5 py-2 rounded-md active:bg-muted">
              {hit.icon ? <Text className="w-4 text-center text-[14px]">{hit.icon}</Text> : <Icon name="docs" size={16} color={muted} />}
              <View className="flex-1 min-w-0">
                <Highlighted text={hit.title} ranges={hit.titleHighlights} className="font-noto-medium text-[14px] leading-[19px]" matchClassName="font-noto-semibold text-[14px] leading-[19px]" />
                {hit.snippet ? <Highlighted text={hit.snippet.text} ranges={hit.snippet.highlights} className="text-[12px] leading-[16px] text-muted-foreground" matchClassName="font-noto-semibold text-[12px] leading-[16px] text-foreground" /> : null}
              </View>
              <Text numberOfLines={1} className="max-w-[40%] text-[11px] text-muted-foreground/70">
                {hit.hint}
              </Text>
            </Pressable>
          )}
        />
      )}
    </SafeAreaView>
  )
}

/**
 * One line of text with the server's matched words in semibold, as the web palette's `Highlighted`. Uniwind does not
 * merge conflicting classes, so a match gets its own full set.
 */
function Highlighted({ text, ranges, className, matchClassName }: { text: string; ranges: TextRange[] | undefined; className: string; matchClassName: string }) {
  return (
    <Text numberOfLines={1} className={className}>
      {splitByRanges(text, (ranges ?? []).map((range) => [range.start, range.end])).map((segment, index) =>
        segment.match ? (
          <Text key={index} className={matchClassName}>
            {segment.text}
          </Text>
        ) : (
          segment.text
        ),
      )}
    </Text>
  )
}
