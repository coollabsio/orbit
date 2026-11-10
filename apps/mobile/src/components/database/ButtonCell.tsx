/** A button property's cell (spec P3 17.4): the web's `ButtonCell`. Its config is edited on the web. */
import type { DatabaseButtonPress } from '@orbit/api/generated/types.gen'
import { buttonLabel } from '@orbit/database/buttons'
import { dayOf } from '@orbit/database/dates'
import type { Property, Row } from '@orbit/database/types'
import { useQueryClient } from '@tanstack/react-query'
import { createContext, useContext } from 'react'
import { Alert, Linking } from 'react-native'

import * as api from '../../data/database-api'
import { keys } from '../../data/keys'
import { type Notify, useDatabaseWrite, useWorkspaceReadOnly } from '../../hooks/database'
import { useOrbit } from '../../orbit-context'
import { Button } from '../../ui'

/** The database whose rows the buttons below press. Without it a button cell shows nothing. */
export const ButtonPressContext = createContext<{ databaseId: string; linkId: string; notify: Notify } | null>(null)

export function ButtonCell({ property, row }: { property: Extract<Property, { type: 'button' }>; row: Row }) {
  const target = useContext(ButtonPressContext)
  if (!target) return null
  return <PressButton property={property} row={row} {...target} />
}

function PressButton({ property, row, databaseId, linkId, notify }: { property: Extract<Property, { type: 'button' }>; row: Row; databaseId: string; linkId: string; notify: Notify }) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const readOnly = useWorkspaceReadOnly()
  const press = useDatabaseWrite<string, DatabaseButtonPress>(databaseId, linkId, notify, {
    request: (propertyId) => api.pressButton(client, workspaceId, databaseId, row.id, propertyId, dayOf(new Date())),
    reconcile: (detail, result) => ({ ...detail, rows: detail.rows.map((other) => (other.id === result.row.id ? result.row : other)) }),
    refetch: true,
    error: 'Could not run the button.',
  })
  const label = buttonLabel(property)
  const run = () =>
    press.mutate(property.id, {
      onSuccess: (result) => {
        const others = new Set(result.created_rows.map((created) => created.database_id).filter((id) => id !== databaseId))
        for (const id of others) void queryClient.invalidateQueries({ queryKey: keys.database(workspaceId, id) })
        if (result.open_url) void Linking.openURL(result.open_url).catch(() => notify('Could not open the link.'))
      },
    })
  return (
    <Button
      label={label}
      variant="outline"
      size="xs"
      icon="flash"
      loading={press.isPending}
      disabled={readOnly || press.isPending}
      onPress={() => {
        const question = property.config.confirm
        if (!question) return run()
        Alert.alert(label, question, [
          { text: 'Cancel', style: 'cancel' },
          { text: 'Continue', onPress: run },
        ])
      }}
    />
  )
}
