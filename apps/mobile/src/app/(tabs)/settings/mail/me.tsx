import { useQueryClient } from '@tanstack/react-query'

import { SignaturesCard, SnippetsCard, UndoSendCard, VacationCard } from '../../../../components/settings/mail/MyMailCards'
import { Note, SettingsCard, SettingsShell, WORKSPACE_NAV } from '../../../../components/settings/Shell'
import { mailKeys } from '../../../../data/keys'
import { mailSendingKeys } from '../../../../data/mail-sending-api'
import { useMailEnabled } from '../../../../hooks/mail'
import { useOrbit } from '../../../../orbit-context'

/** The web's Settings → Mail → Mail preferences: each member's signatures, undo send delay, snippets and vacation reply. */
export default function MyMailScreen() {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const mailEnabled = useMailEnabled()
  const refresh = () =>
    Promise.all(
      [mailKeys.fromAddresses(workspaceId).slice(0, 3), mailKeys.me(workspaceId), mailSendingKeys.signatures(workspaceId), mailSendingKeys.settingsMe(workspaceId), mailSendingKeys.snippets(workspaceId), ['mail', workspaceId, 'vacation']].map((queryKey) =>
        queryClient.invalidateQueries({ queryKey }),
      ),
    )
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={mailEnabled ? refresh : undefined}>
      {mailEnabled ? (
        <>
          <SignaturesCard />
          <UndoSendCard />
          <SnippetsCard />
          <VacationCard />
        </>
      ) : (
        <SettingsCard title="Mail preferences">
          <Note>Mail is off on this Orbit server.</Note>
        </SettingsCard>
      )}
    </SettingsShell>
  )
}
