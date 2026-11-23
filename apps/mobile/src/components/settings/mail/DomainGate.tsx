import type { ReactNode } from 'react'

import { useMailEnabled } from '../../../hooks/mail'
import { useOrbit } from '../../../orbit-context'
import { Text } from '../../../ui'
import { SettingsCard } from '../Shell'

/** Whether this user may see the Domains and Providers pages: mail is on and they have `mail.manage`. */
export function useMailManager(): boolean {
  const { workspace } = useOrbit()
  return useMailEnabled() && workspace.permissions.includes('mail.manage')
}

/** The web's `MailManagersOnly`, and a note when mail is off on the server; else `children`. */
export function MailManagerGate({ title, children }: { title: string; children: ReactNode }) {
  const { workspace } = useOrbit()
  const enabled = useMailEnabled()
  if (!enabled) {
    return (
      <SettingsCard title={title}>
        <Text className="text-[14px] leading-[20px] text-muted-foreground">Mail is off on this Orbit server. An instance administrator can turn it on in Admin → Mail.</Text>
      </SettingsCard>
    )
  }
  if (!workspace.permissions.includes('mail.manage')) {
    return (
      <SettingsCard title={title}>
        <Text className="text-[14px] leading-[20px] text-muted-foreground">Only workspace owners and administrators can manage mail.</Text>
      </SettingsCard>
    )
  }
  return children
}
