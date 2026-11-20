import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { Linking, View } from 'react-native'

import { whileMounted } from '../../data/page-editor'
import { FRAME_MIN_HEIGHT, frameDocument, isOpenableLink } from '../../data/mail-frame'
import { useViewDocument } from '../../hooks/mail'
import { useOrbit } from '../../orbit-context'
import { cssColors, useTheme } from '../../theme'
import { Button, Spinner, Text } from '../../ui'
import MailHtml from './MailHtml.dom'

/**
 * A message's HTML body: the view document fetched natively, framed by the DOM component at the height its frame
 * reports, so the thread scrolls natively and the web view never does. A link opens in the system browser.
 */
export const MailHtmlView = memo(function MailHtmlView({ messageId, loadImages, onBlockedImages }: { messageId: string; loadImages: boolean; onBlockedImages: (count: number) => void }) {
  const { session } = useOrbit()
  const { scheme, colors } = useTheme()
  const view = useViewDocument(messageId, loadImages)
  const [height, setHeight] = useState(FRAME_MIN_HEIGHT)
  const latest = useRef(onBlockedImages)
  useEffect(() => {
    latest.current = onBlockedImages
  })
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])
  const document = useMemo(() => (view.data ? frameDocument(view.data.html, view.data.policy, session.server) : null), [view.data, session.server])
  const actions = useMemo(
    () => ({
      onSize: whileMounted(mounted, async (size: { height: number; blocked: number | null }) => {
        setHeight(size.height)
        if (size.blocked !== null) latest.current(size.blocked)
      }),
      // the frame filters nothing: the app checks the address before it hands it to the system
      onOpenLink: whileMounted(mounted, async (url: string) => {
        if (isOpenableLink(url)) void Linking.openURL(url).catch(() => {})
      }),
    }),
    [],
  )
  const dom = useMemo(() => ({ scrollEnabled: false, style: { height } }), [height])

  if (view.isError) {
    return (
      <View accessibilityRole="alert" className="flex-row items-center gap-3 rounded-md border border-border px-3 py-2">
        <Text className="flex-1 text-[13px] text-muted-foreground">The message could not be shown.</Text>
        <Button label="Try again" variant="outline" size="sm" onPress={() => void view.refetch()} />
      </View>
    )
  }
  if (!document) {
    return (
      <View className="h-20 items-center justify-center rounded-md bg-muted">
        <Spinner />
      </View>
    )
  }
  return (
    <View className="overflow-hidden rounded-md" style={{ height }}>
      <MailHtml dom={dom} document={document} theme={scheme} background={colors.background} paper={cssColors.white} {...actions} />
    </View>
  )
})
