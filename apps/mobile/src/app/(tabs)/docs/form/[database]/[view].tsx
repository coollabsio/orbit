import { repairConfig } from '@orbit/database/view'
import { router, useLocalSearchParams } from 'expo-router'
import { View } from 'react-native'

import { FormFill } from '../../../../../components/database/FormFill'
import { useDatabase, useViewContext, useWorkspaceReadOnly } from '../../../../../hooks/database'
import { alpha, useTheme } from '../../../../../theme'
import { ErrorView, IconButton, LoadingView, PaneHeader, SafeAreaView, Text } from '../../../../../ui'

/**
 * The native screen of a form link (the web's `/docs/<database>/form/<view>`, spec P3 18.2): fill in and submit.
 * `link` reads the view from a linked view block's views instead of the database's own.
 */
export default function FormScreen() {
  const { colors } = useTheme()
  const params = useLocalSearchParams<{ database: string; view: string; link?: string }>()
  const databaseId = params.database ?? ''
  const query = useDatabase(databaseId, params.link ?? '')
  const ctx = useViewContext()
  const readOnly = useWorkspaceReadOnly()
  const database = query.data
  const view = database?.views.find((candidate) => candidate.id === params.view && candidate.type === 'form')

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <IconButton icon="back" label="Back" color={alpha(colors.mutedForeground, 0.7)} onPress={() => (router.canGoBack() ? router.back() : router.navigate(`/docs/${databaseId}`))} />
        <Text numberOfLines={1} className="font-noto-semibold text-[13px] flex-1 min-w-0">
          {database?.database.title || 'Form'}
        </Text>
      </PaneHeader>
      {database && view ? (
        <FormFill database={database} viewId={view.id} config={repairConfig(view, database.properties)} ctx={ctx} canSubmit={!readOnly} />
      ) : query.isError ? (
        <ErrorView error={query.error} onRetry={() => void query.refetch()} />
      ) : database ? (
        <View className="items-center gap-1 px-6 py-10">
          <Text className="text-[14px] text-muted-foreground">Form not found</Text>
          <Text className="text-center text-[12px] text-muted-foreground">This form was deleted.</Text>
        </View>
      ) : (
        <LoadingView />
      )}
    </SafeAreaView>
  )
}
