import type { ReactNode } from 'react'
import { ScrollView, View } from 'react-native'
import type { Edge } from 'react-native-safe-area-context'

import { SafeAreaView } from './SafeAreaView'

export interface ScreenProps {
  children: ReactNode
  /** Safe-area edges to pad. Screens under a navigator header or tab bar pass only the edges those bars leave uncovered. */
  edges?: readonly Edge[]
  scroll?: boolean
  /** Centers the content, for forms and empty states. */
  centered?: boolean
}

export function Screen({ children, edges = ['top', 'bottom', 'left', 'right'], scroll, centered }: ScreenProps) {
  const content = `grow gap-3 p-4 ${centered ? 'justify-center' : ''}`
  return (
    <SafeAreaView edges={edges} className="flex-1 bg-background">
      {scroll ? (
        <ScrollView contentContainerClassName={content} keyboardShouldPersistTaps="handled" contentInsetAdjustmentBehavior="automatic">
          {children}
        </ScrollView>
      ) : (
        <View className={content}>{children}</View>
      )}
    </SafeAreaView>
  )
}
