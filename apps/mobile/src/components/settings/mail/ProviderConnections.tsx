/**
 * The web's `MailConnections` (`MailProvidersPage.tsx`): one card for the providers of a scope. Incoming providers
 * receive a domain's mail and pass it to Orbit; outgoing routes send it. Settings → Mail → Providers uses the
 * `workspace` scope, Admin → Mail the `instance` one.
 */
import { useState } from 'react'
import { View } from 'react-native'

import type { MailProviderView, ProviderScope } from '../../../data/mail-domains-api'
import { count, mailProblem, PROVIDER_LABELS, since } from '../../../data/mail-settings'
import { useMailProviders, useProviderWrites } from '../../../hooks/mail-domains'
import { useTheme } from '../../../theme'
import { Button, Icon, Text } from '../../../ui'
import { ActionSheet, ConfirmDialog, type ConfirmRequest, Dialog } from '../Dialog'
import { Badge, Note, SettingsCard } from '../Shell'
import { ProviderForm } from './ProviderForm'
import { ProviderSetup } from './ProviderSetup'
import { OutgoingRoutes } from './SendRouteList'

type Direction = 'incoming' | 'outgoing'

/** **Add provider** asks which one. `hostname` fills the SPF of a direct route. */
export function ProviderConnections({ scope, hostname }: { scope: ProviderScope; hostname?: string | null }) {
  const { colors } = useTheme()
  const providers = useMailProviders(scope)
  const [adding, setAdding] = useState<Direction | null>(null)
  const [menu, setMenu] = useState(false)
  const turnedOff = scope === 'workspace' && providers.data !== undefined && !providers.data.workspace_providers_enabled
  const canAdd = scope === 'instance' || providers.data?.workspace_providers_enabled === true

  return (
    <>
      {turnedOff ? (
        <View accessibilityRole="alert" className="gap-1 rounded-lg border border-border bg-card p-3">
          <View className="flex-row items-center gap-1.5">
            <Icon name="infoCircle" size={16} color={colors.foreground} />
            <Text className="min-w-0 flex-1 font-noto-medium text-[13px] text-foreground">Your server admin has turned off workspace providers.</Text>
          </View>
          <Text className="text-[13px] leading-[19px] text-muted-foreground">Your domains can still use the providers your server admin set up, listed below.</Text>
        </View>
      ) : null}
      <SettingsCard
        title="Providers"
        description={
          scope === 'instance'
            ? 'Incoming providers receive mail for the domains and pass it to Orbit: use one when this server cannot receive on port 25. Outgoing providers send mail: directly from this server, through an SMTP relay or through a provider. Every workspace can pick these for its domains. Domains without an outgoing provider send through the default one; without a default, they cannot send.'
            : 'Incoming providers receive mail for your domains through your own account, when this server cannot receive on port 25. Outgoing providers send it. Pick them for each domain on its card in Domains; without an outgoing one, a domain uses the server’s default.'
        }
        actions={canAdd ? <Button label="Add provider" icon="plus" size="sm" onPress={() => setMenu(true)} /> : undefined}
        flush
      >
        <SectionHeading>Incoming</SectionHeading>
        <IncomingProviders scope={scope} adding={adding === 'incoming'} onAddClose={() => setAdding(null)} />
        <SectionHeading divided>Outgoing</SectionHeading>
        <OutgoingRoutes scope={scope} hostname={hostname} adding={adding === 'outgoing'} onAddClose={() => setAdding(null)} />
      </SettingsCard>
      <ActionSheet
        visible={menu}
        title="Add provider"
        onClose={() => setMenu(false)}
        actions={[
          { label: 'Incoming: receive mail', icon: 'directInbox', onPress: () => setAdding('incoming') },
          { label: 'Outgoing: send mail', icon: 'send', onPress: () => setAdding('outgoing') },
        ]}
      />
    </>
  )
}

function SectionHeading({ children, divided = false }: { children: string; divided?: boolean }) {
  return (
    <View className={`border-b border-border bg-muted/40 px-4 py-2 ${divided ? 'border-t' : ''}`}>
      <Text accessibilityRole="header" className="font-noto-medium text-[12px] text-muted-foreground">
        {children}
      </Text>
    </View>
  )
}

/** The incoming providers of `scope`. Each own provider shows what to set up at the provider; secrets Orbit makes show once. */
function IncomingProviders({ scope, adding, onAddClose }: { scope: ProviderScope; adding: boolean; onAddClose: () => void }) {
  const providers = useMailProviders(scope)
  const [editing, setEditing] = useState<MailProviderView | null>(null)
  // a provider with secrets that Orbit just made: shown once
  const [shown, setShown] = useState<{ provider: MailProviderView; title: string } | null>(null)

  return (
    <View>
      {providers.isPending ? (
        <View className="p-4">
          <Note loading>Loading providers…</Note>
        </View>
      ) : null}
      {providers.isError ? (
        <View className="flex-row flex-wrap items-center gap-2 p-4">
          <Note error>{mailProblem(providers.error, 'The providers could not be loaded.')}</Note>
          <Button label="Retry" variant="ghost" size="sm" onPress={() => void providers.refetch()} />
        </View>
      ) : null}
      {providers.data ? <ListenerRow running={providers.data.listener.running} hostname={providers.data.listener.hostname} /> : null}
      {providers.data?.providers.map((provider) => (
        <ProviderRow key={provider.id} scope={scope} provider={provider} onEdit={() => setEditing(provider)} onNewSecret={(changed) => setShown({ provider: changed, title: `New secret for ${changed.name}` })} />
      ))}
      {adding ? <ProviderForm scope={scope} onClose={onAddClose} onCreated={(created) => setShown({ provider: created, title: `Set up ${created.name}` })} /> : null}
      {editing ? <ProviderForm scope={scope} provider={editing} onClose={() => setEditing(null)} /> : null}
      {shown?.provider.setup ? (
        <Dialog visible title={shown.title} description={`Do these steps at ${PROVIDER_LABELS[shown.provider.kind]}.`} onClose={() => setShown(null)} footer={<Button label="Done" onPress={() => setShown(null)} />}>
          <ProviderSetup setup={shown.provider.setup} generated={shown.provider.generated_secrets} />
        </Dialog>
      ) : null}
    </View>
  )
}

/** This server's SMTP listener: domains without an incoming provider receive through it. */
function ListenerRow({ running, hostname }: { running: boolean; hostname?: string | null }) {
  return (
    <View className="gap-0.5 px-4 py-3">
      <View className="flex-row flex-wrap items-center gap-2">
        <Text className="font-noto-medium text-[13px] text-foreground">SMTP listener</Text>
        <Badge label="Server" tone="secondary" />
        <Badge label={running ? 'Running' : 'Off'} tone={running ? 'outline' : 'destructive'} />
      </View>
      <Text className="text-[12px] leading-[17px] text-muted-foreground">
        {running ? `Receives mail for domains without an incoming provider${hostname ? ` (MX: ${hostname})` : ''}.` : 'This server does not receive mail itself. Domains need an incoming provider.'}
      </Text>
    </View>
  )
}

function ProviderRow({ scope, provider, onEdit, onNewSecret }: { scope: ProviderScope; provider: MailProviderView; onEdit: () => void; onNewSecret: (changed: MailProviderView) => void }) {
  const { remove, update } = useProviderWrites(scope)
  // Orbit makes these secrets, so Orbit can replace them
  const madeByOrbit = provider.kind === 'cloudflare' || provider.kind === 'postmark'
  const [open, setOpen] = useState(false)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  // the web's toasts
  const [problem, setProblem] = useState<string | null>(null)
  const own = provider.setup !== null && provider.setup !== undefined

  const removeProvider = () =>
    setConfirm({
      title: `Remove ${provider.name}?`,
      description: 'Orbit stops accepting mail from it. Remove it at the provider too.',
      confirmLabel: 'Remove',
      danger: true,
      onConfirm: () => {
        setProblem(null)
        remove.mutate(provider.id, { onError: (error) => setProblem(mailProblem(error, 'The provider could not be removed.')) })
      },
    })

  const newSecret = () =>
    setConfirm({
      title: `Make a new secret for ${provider.name}?`,
      description: provider.kind === 'cloudflare' ? 'The old signing secret stops working now. Mail is refused until you set the new one on the Worker.' : 'The old credentials stop working now. Mail is refused until you set the new webhook URL in Postmark.',
      confirmLabel: 'Make new secret',
      danger: true,
      onConfirm: () => {
        setProblem(null)
        update.mutate({ providerId: provider.id, body: { regenerate_secrets: true } }, { onSuccess: onNewSecret, onError: (error) => setProblem(mailProblem(error, 'The new secret could not be made.')) })
      },
    })

  return (
    <View className="gap-2 border-t border-border px-4 py-3">
      <View className="gap-0.5">
        <View className="flex-row flex-wrap items-center gap-2">
          <Text className="font-noto-medium text-[13px] text-foreground">{provider.name}</Text>
          <Badge label={PROVIDER_LABELS[provider.kind]} tone="outline" />
          {scope === 'workspace' && provider.scope === 'instance' ? <Badge label="Server" tone="secondary" /> : null}
        </View>
        <Text className="text-[12px] leading-[17px] text-muted-foreground">
          {count(provider.domain_count, 'domain', 'domains')}
          {provider.last_received_at ? ` · Last message ${since(provider.last_received_at)}` : ' · No message yet'}
        </Text>
        {provider.last_error ? <Text className="text-[12px] leading-[17px] text-destructive">Last error: {provider.last_error}</Text> : null}
      </View>
      {own ? (
        <View className="-ml-2 flex-row flex-wrap items-center gap-1">
          <Button label={open ? 'Hide setup' : 'Setup'} variant="outline" size="sm" onPress={() => setOpen(!open)} />
          <Button label="Change" variant="ghost" size="sm" onPress={onEdit} />
          {madeByOrbit ? <Button label="New secret" variant="ghost" size="sm" disabled={update.isPending} onPress={newSecret} /> : null}
          <Button label="Remove" variant="ghost" size="sm" disabled={remove.isPending} onPress={removeProvider} />
        </View>
      ) : null}
      {problem ? <Note error>{problem}</Note> : null}
      {open && provider.setup ? <ProviderSetup setup={provider.setup} provider={provider} scope={scope} /> : null}
    </View>
  )
}
