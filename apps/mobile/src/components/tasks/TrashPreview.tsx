import { router } from 'expo-router'
import type { ReactNode } from 'react'
import { Alert, ScrollView, View } from 'react-native'

import { alpha, useTheme } from '../../theme'
import { EmptyState, IconButton, LoadingView, PaneHeader, SafeAreaView, Text } from '../../ui'

const backToTrash = () => (router.canGoBack() ? router.back() : router.navigate('/tasks/trash'))

/** A read-only preview of a trashed record: back to the trash, the red trash banner, then the record. */
export function TrashPreview({ title, banner, error, loading, gone, children }: { title: string; banner?: ReactNode; error?: string | null; loading?: boolean; gone?: 'task' | 'project'; children?: ReactNode }) {
  const { colors } = useTheme()
  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <IconButton icon="back" label="Back to trash" color={alpha(colors.mutedForeground, 0.7)} onPress={backToTrash} />
        <Text numberOfLines={1} className="font-noto-semibold text-[13px] flex-1 min-w-0">
          {title ? `Trash / ${title}` : 'Trash'}
        </Text>
      </PaneHeader>
      {gone ? (
        <EmptyState icon="trash" title={`This ${gone} is not in the trash`} description="It was restored or deleted forever." />
      ) : loading ? (
        <LoadingView />
      ) : (
        <>
          {banner}
          {error ? <Text className="text-[13px] mx-4 mt-2 text-destructive">{error}</Text> : null}
          <ScrollView contentContainerClassName="px-3.5 pt-4 pb-7">{children}</ScrollView>
        </>
      )}
    </SafeAreaView>
  )
}

/** The web's red banner: who trashed it and when it goes, with its actions. */
export function TrashBanner({ text, children }: { text: string; children: ReactNode }) {
  return (
    <View accessibilityRole="summary" className="gap-2 border-b border-destructive/20 bg-destructive/10 px-4 py-2.5">
      <Text className="text-[13px] leading-[18px] text-destructive">{text}</Text>
      <View className="flex-row flex-wrap gap-2">{children}</View>
    </View>
  )
}

/** The native confirm before Delete forever. */
export function confirmDeleteForever(name: string, description: string, onConfirm: () => void) {
  Alert.alert(`Delete “${name}” forever?`, description, [
    { text: 'Cancel', style: 'cancel' },
    { text: 'Delete forever', style: 'destructive', onPress: onConfirm },
  ])
}

/** A label and its value in the preview's property list. */
export function Property({ name, children }: { name: string; children: ReactNode }) {
  return (
    <View className="flex-row items-center gap-3 min-h-7">
      <Text className="w-24 text-[13px] text-muted-foreground">{name}</Text>
      <View className="flex-1 min-w-0 flex-row flex-wrap items-center gap-1.5">{children}</View>
    </View>
  )
}
