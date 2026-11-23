import { Callout } from '../../../../components/settings/mail/SendRouteForm'
import { PolicyRows } from '../../../../components/settings/mail/SendRouteList'
import { Note, SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../../components/settings/Shell'
import { mailProblem, since } from '../../../../data/mail-settings'
import { useMailEnabled, useSendPolicy } from '../../../../hooks/mail'
import { useBillingEnabled } from '../../../../hooks/workspace-settings'
import { useOrbit } from '../../../../orbit-context'

/** The web's Settings → Mail → Sending: the workspace's tier and limits, and a paused banner. Its routes are in Providers. */
export default function MailSendingScreen() {
  const { workspace } = useOrbit()
  const mailEnabled = useMailEnabled()
  const canManage = workspace.permissions.includes('mail.manage')
  const policy = useSendPolicy(mailEnabled && canManage)
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={mailEnabled && canManage ? () => policy.refetch() : undefined}>
      {!mailEnabled ? (
        <SettingsCard title="Sending">
          <Note>Mail is off on this Orbit server.</Note>
        </SettingsCard>
      ) : !canManage ? (
        <SettingsCard title="Sending">
          <Note>Only workspace owners and administrators can manage mail.</Note>
        </SettingsCard>
      ) : (
        <>
          <PausedAlert />
          <SendLimitsCard />
        </>
      )}
    </SettingsShell>
  )
}

function PausedAlert() {
  const policy = useSendPolicy(true)
  if (!policy.data?.paused_at) return null
  return (
    <Callout
      destructive
      title="Sending is paused for this workspace"
      description={`${policy.data.pause_reason ? `${policy.data.pause_reason} ` : ''}Paused ${since(policy.data.paused_at)}. New messages wait in the Outbox until the server admin resumes sending.`}
    />
  )
}

function SendLimitsCard() {
  const policy = useSendPolicy(true)
  const cloud = useBillingEnabled()
  return (
    <SettingsCard
      title="Limits"
      description={
        cloud
          ? 'Recipients are counted, not messages. A new workspace starts on New and moves to Standard after two weeks of good sending.'
          : 'Recipients are counted, not messages. Your server admin sets the limits of each workspace.'
      }
      flush
    >
      {policy.isPending ? (
        <SettingsRow first>
          <Note loading>Loading limits…</Note>
        </SettingsRow>
      ) : null}
      {policy.isError ? (
        <SettingsRow first>
          <Note error>{mailProblem(policy.error, 'The limits could not be loaded.')}</Note>
        </SettingsRow>
      ) : null}
      {policy.data ? <PolicyRows policy={policy.data} /> : null}
    </SettingsCard>
  )
}
