import { ApiProblem } from '@orbit/api/problem'
import { useQueryClient } from '@tanstack/react-query'
import { router } from 'expo-router'
import { useEffect, useState } from 'react'
import { Alert, View } from 'react-native'

import { mailKeys } from '../../data/keys'
import * as mail from '../../data/mail-api'
import { refreshSendViews, setPendingSend, usePendingSend } from '../../hooks/mail'
import { useOrbit } from '../../orbit-context'
import { errorMessage } from '../../session-context'
import { Button, Text } from '../../ui'

/**
 * "Sending… Undo" while the server holds a message the user just sent (the undo time of Settings → Mail). Undo stops
 * it on the server and opens its draft again; after the time it is gone.
 */
export function UndoSendBar() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const pending = usePendingSend()
  const [now, setNow] = useState(Date.now)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    if (!pending) return
    const timer = setInterval(() => setNow(Date.now()), 500)
    return () => clearInterval(timer)
  }, [pending])
  useEffect(() => {
    if (pending && now >= pending.releaseAt) setPendingSend(null)
  }, [pending, now])
  if (!pending || now >= pending.releaseAt) return null
  const seconds = Math.max(1, Math.ceil((pending.releaseAt - now) / 1000))

  const undo = async () => {
    setBusy(true)
    try {
      const draft = await mail.cancelOutbound(client, workspaceId, pending.outboundId)
      setPendingSend(null)
      queryClient.setQueryData(mailKeys.draft(workspaceId, draft.id), draft)
      void refreshSendViews(queryClient, workspaceId)
      router.push({ pathname: '/mail/compose', params: { draft: draft.id } })
    } catch (error) {
      setPendingSend(null)
      Alert.alert(error instanceof ApiProblem && error.code === 'already_sent' ? 'Too late: the message was already sent.' : 'The message could not be stopped. Check Sent.', error instanceof ApiProblem && error.code === 'already_sent' ? undefined : errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  return (
    <View accessibilityLiveRegion="polite" className="mx-3 mb-2 flex-row items-center gap-3 rounded-lg border border-border bg-popover px-3 py-2">
      <Text className="flex-1 text-[13px]">Sending in {seconds} s</Text>
      <Button label="Undo" variant="outline" size="sm" loading={busy} onPress={() => void undo()} />
    </View>
  )
}
