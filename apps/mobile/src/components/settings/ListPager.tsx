import { useState } from 'react'
import { Pressable, View } from 'react-native'

import { useTheme } from '../../theme'
import { Icon, Text } from '../../ui'
import { Select } from './Shell'

const PAGE_SIZES = [10, 25, 50, 100].map((size) => ({ value: String(size), label: String(size) }))

/** Client-side paging of a list, as on the web. `visible` holds the rows of the current page. */
export function usePagedList<T>(items: T[], initialPerPage = 10) {
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(initialPerPage)
  const lastPage = Math.max(1, Math.ceil(items.length / perPage))
  const currentPage = Math.min(page, lastPage)
  return {
    visible: items.slice((currentPage - 1) * perPage, currentPage * perPage),
    pager: { total: items.length, page: currentPage, lastPage, perPage, setPage, setPerPage },
    resetPage: () => setPage(1),
  }
}

type PagerState = ReturnType<typeof usePagedList>['pager']

/** The web's list footer: the row range, the page size and the previous and next buttons. */
export function ListPager({ total, page, lastPage, perPage, setPage, setPerPage }: PagerState) {
  const firstRow = total === 0 ? 0 : (page - 1) * perPage + 1
  return (
    <View className="min-h-11 flex-row items-center justify-between px-4">
      <View className="flex-row items-center gap-3">
        <Text className="text-[11px] text-muted-foreground/70">{`${firstRow}-${Math.min(page * perPage, total)} of ${total}`}</Text>
        <View className="w-16">
          <Select label="Items per page" size="sm" value={String(perPage)} options={PAGE_SIZES} onChange={(value) => { setPerPage(Number(value)); setPage(1) }} />
        </View>
      </View>
      <View className="flex-row items-center gap-1">
        <PageButton previous label="Previous page" disabled={page <= 1} onPress={() => setPage(page - 1)} />
        <PageButton label="Next page" disabled={page >= lastPage} onPress={() => setPage(page + 1)} />
      </View>
    </View>
  )
}

function PageButton({ previous = false, label, disabled, onPress }: { previous?: boolean; label: string; disabled: boolean; onPress: () => void }) {
  const { colors } = useTheme()
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      disabled={disabled}
      onPress={onPress}
      className={`size-7 items-center justify-center rounded-md border border-border bg-background active:bg-muted dark:border-input dark:bg-input/30 ${disabled ? 'opacity-50' : ''}`}
    >
      <View className={previous ? 'rotate-180' : ''}>
        <Icon name="arrowRight" size={14} color={colors.foreground} />
      </View>
    </Pressable>
  )
}
