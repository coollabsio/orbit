import type { ReactNode } from 'react'

import { useMailEnabled } from '../../../hooks/mail'
import { useOrbit } from '../../../orbit-context'
import { Note, SettingsCard, SettingsShell, WORKSPACE_NAV } from '../Shell'

/**
 * A Settings → Mail manager page (Mailboxes, Mail labels, Suppressions, Failed messages): the web's `MailGate` and
 * `MailManagersOnly`. `children` render only while Mail is on and the user holds `mail.manage`.
 */
export function MailManagerPage({ title, onRefresh, children }: { title: string; onRefresh?: () => Promise<unknown>; children: ReactNode }) {
  const { workspace } = useOrbit()
  const mailOn = useMailEnabled()
  const canManage = workspace.permissions.includes('mail.manage')
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={mailOn && canManage ? onRefresh : undefined}>
      {!mailOn ? (
        <SettingsCard title={title}>
          <Note>Mail is off on this Orbit server. An instance administrator can turn it on in Admin → Mail.</Note>
        </SettingsCard>
      ) : !canManage ? (
        <SettingsCard title={title}>
          <Note>Only workspace owners and administrators can manage mail.</Note>
        </SettingsCard>
      ) : (
        children
      )}
    </SettingsShell>
  )
}
