import { viewCsv, viewCsvName, viewProperties } from '@orbit/database/csv'
import type { Database, DatabaseView, Row, ViewContext } from '@orbit/database/types'
import { File, Paths } from 'expo-file-system'
import { Platform, Share } from 'react-native'

/**
 * "Export view as CSV" (spec P2 9.5): the rows the view shows under its visible properties, handed to the share sheet as
 * a file on iOS and as text on Android (its share sheet takes no file URLs); the web build downloads it.
 */
export async function shareViewCsv(database: Database, view: DatabaseView, rows: readonly Row[], ctx: ViewContext): Promise<void> {
  const text = viewCsv(viewProperties(database, view), rows, ctx)
  const name = viewCsvName(database, view)
  if (Platform.OS === 'web') {
    const url = URL.createObjectURL(new Blob([text], { type: 'text/csv;charset=utf-8' }))
    const link = document.createElement('a')
    link.href = url
    link.download = name
    link.click()
    setTimeout(() => URL.revokeObjectURL(url), 10_000)
    return
  }
  if (Platform.OS === 'android') {
    await Share.share({ title: name, message: text })
    return
  }
  const file = new File(Paths.cache, name)
  if (file.exists) file.delete()
  file.create()
  file.write(text)
  await Share.share({ url: file.uri, title: name })
}
