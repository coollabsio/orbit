import { useState } from 'react'
import { View } from 'react-native'

import { AdminShell, Badge, Note, SettingsCard, SettingsRow } from '../../../../components/settings/Shell'
import { openFile } from '../../../../components/tasks/files'
import { AUDIT_EXPORT_PATH, auditActor } from '../../../../data/admin-api'
import { useAuditEvents } from '../../../../hooks/admin'
import { useOrbit } from '../../../../orbit-context'
import { errorMessage } from '../../../../session-context'
import { Button, Text } from '../../../../ui'

/**
 * The web's Admin → Audit log, newest first. Export CSV downloads the file with the session token and opens it in the
 * system preview, where the share button saves or sends it.
 */
export default function AdminAuditScreen() {
  const { session } = useOrbit()
  const audit = useAuditEvents()
  const [exporting, setExporting] = useState(false)
  const [exportError, setExportError] = useState<string | null>(null)
  const exportCsv = () => {
    setExporting(true)
    setExportError(null)
    openFile(`${session.server}${AUDIT_EXPORT_PATH}`, 'orbit-audit.csv', session.token)
      .catch((error: unknown) => setExportError(errorMessage(error)))
      .finally(() => setExporting(false))
  }
  const events = audit.data?.pages.flatMap((page) => page.items) ?? []
  return (
    <AdminShell refreshing={audit.isRefetching} onRefresh={() => void audit.refetch()}>
      <SettingsCard
        title="Audit log"
        description="Sign-ins, account changes and other security events from every workspace."
        actions={<Button label={exporting ? 'Exporting…' : 'Export CSV'} icon="download" variant="outline" size="sm" disabled={exporting} onPress={exportCsv} />}
        flush
      >
        {exportError ? <View className="p-4 border-b border-border"><Note error>The export could not be opened. {exportError}</Note></View> : null}
        {audit.isPending ? <View className="p-4"><Note>Loading events…</Note></View> : null}
        {audit.isError ? (
          <View className="p-4 flex-row items-center gap-2">
            <Note error>Events could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void audit.refetch()} />
          </View>
        ) : null}
        {audit.data && events.length === 0 ? <View className="p-4"><Note>No events yet.</Note></View> : null}
        {events.map((event, index) => (
          <SettingsRow key={event.id} first={index === 0}>
            <View className="flex-1 min-w-0 gap-0.5">
              <View className="flex-row items-center gap-1.5">
                <Text numberOfLines={1} className="shrink font-mono text-[12px] text-foreground leading-[21px]">
                  {event.action}
                </Text>
                {event.outcome === 'failure' ? <Badge label="Failed" tone="destructive" /> : null}
              </View>
              <Text numberOfLines={1} className="text-[12px] text-muted-foreground">{`${auditActor(event)} · ${new Date(event.occurred_at).toLocaleString()}`}</Text>
            </View>
          </SettingsRow>
        ))}
        {audit.hasNextPage ? (
          <View className="p-2 items-center border-t border-border">
            <Button label={audit.isFetchingNextPage ? 'Loading…' : 'Load older events'} variant="ghost" disabled={audit.isFetchingNextPage} onPress={() => void audit.fetchNextPage()} />
          </View>
        ) : null}
      </SettingsCard>
    </AdminShell>
  )
}
