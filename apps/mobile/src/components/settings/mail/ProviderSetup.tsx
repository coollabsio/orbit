/** The web's `ProviderSetup.tsx`: what to set up at an incoming provider, and **Also send through this provider**. */
import { useState } from 'react'
import { ScrollView, View } from 'react-native'

import type { MailProviderSetup } from '@orbit/api/generated/types.gen'
import type { MailProviderView, ProviderScope } from '../../../data/mail-domains-api'
import type { MailSendRouteView } from '../../../data/mail-sending-api'
import { useMailSendRoutes } from '../../../hooks/mail-domains'
import { useTheme } from '../../../theme'
import { Button, Icon, LabelTip, Text } from '../../../ui'
import { CopyButton } from './DomainRecords'
import { ProviderSendRouteForm } from './SendRouteForm'
import { RouteCreatedDialog } from './SendRouteList'

const SECRET_LABELS: Record<string, string> = {
  signing_secret: 'Signing secret',
  basic_user: 'User name',
  basic_password: 'Password',
}

/** One value to paste at the provider, with a copy button. */
function CopyRow({ label, value }: { label: string; value: string }) {
  return (
    <View className="gap-1">
      <Text className="font-noto-medium text-[12px] text-muted-foreground">{label}</Text>
      <View className="flex-row items-start gap-1 rounded-md border border-border bg-muted/40 px-2 py-1">
        <Text selectable className="min-w-0 flex-1 py-0.5 font-mono text-[12px] leading-[17px]">
          {value}
        </Text>
        <CopyButton text={value} label={label} />
      </View>
    </View>
  )
}

/**
 * What to set up at the provider: the webhook URL, the steps and (Cloudflare) the Email Worker. `generated` holds the
 * secrets Orbit made, which only the create answer has. With `provider`: **Also send through this provider**.
 */
export function ProviderSetup({ setup, generated, provider, scope }: { setup: MailProviderSetup; generated?: Record<string, string> | null; provider?: MailProviderView; scope?: ProviderScope }) {
  const { colors } = useTheme()
  const secrets = Object.entries(generated ?? {})
  return (
    <View className="gap-4">
      {secrets.length > 0 ? (
        <View accessibilityRole="alert" className="gap-1 rounded-lg border border-border bg-card p-3">
          <View className="flex-row items-center gap-1.5">
            <Icon name="danger" size={16} color={colors.foreground} />
            <Text className="min-w-0 flex-1 font-noto-medium text-[13px] text-foreground">Copy these now. Orbit does not show them again.</Text>
          </View>
          <Text className="text-[12px] leading-[17px] text-muted-foreground">If you lose them, remove the provider and add it again.</Text>
        </View>
      ) : null}
      <CopyRow label="Webhook URL" value={setup.webhook_url} />
      {secrets.map(([name, value]) => (
        <CopyRow key={name} label={SECRET_LABELS[name] ?? name} value={value} />
      ))}
      <View className="gap-1.5">
        {setup.steps.map((step, index) => (
          <View key={step} className="flex-row gap-1.5">
            <Text className="w-4 text-[13px] leading-[19px] text-muted-foreground">{`${index + 1}.`}</Text>
            <Text className="min-w-0 flex-1 text-[13px] leading-[19px] text-foreground">{step}</Text>
          </View>
        ))}
      </View>
      {setup.worker_script ? (
        <View className="gap-1">
          <View className="flex-row items-center justify-between gap-2">
            <Text className="font-noto-medium text-[12px] text-muted-foreground">Email Worker script</Text>
            <CopyButton text={setup.worker_script} label="Script" />
          </View>
          <ScrollView nestedScrollEnabled className="max-h-72 rounded-md border border-border bg-muted/40">
            <Text selectable className="p-2 font-mono text-[12px] leading-[17px]">
              {setup.worker_script}
            </Text>
          </ScrollView>
        </View>
      ) : null}
      {provider && scope ? <ProviderSendRoute provider={provider} scope={scope} /> : null}
    </View>
  )
}

/** **Also send through this provider**, or the route that already does. */
function ProviderSendRoute({ provider, scope }: { provider: MailProviderView; scope: ProviderScope }) {
  const routes = useMailSendRoutes(scope)
  const [adding, setAdding] = useState(false)
  const [created, setCreated] = useState<MailSendRouteView | null>(null)
  if (!routes.data) return null
  const route = routes.data.routes.find((entry) => entry.provider_id === provider.id)
  const allowed = scope === 'instance' || routes.data.workspace_providers_enabled
  if (!route && !allowed) return null
  return (
    <View className="gap-2 border-t border-border pt-3">
      <LabelTip
        label={route ? `Also sends mail through the route ${route.name}` : 'Send mail too'}
        className="font-noto-medium text-[13px] text-foreground"
        text="A send route with the same account, so your domains can send through this provider. Orbit asks only for what the provider does not have yet."
      />
      {route ? null : (
        <View className="flex-row">
          <Button label="Also send through this provider" variant="outline" size="sm" onPress={() => setAdding(true)} />
        </View>
      )}
      {adding ? <ProviderSendRouteForm scope={scope} provider={provider} onClose={() => setAdding(false)} onCreated={setCreated} /> : null}
      {created ? <RouteCreatedDialog created={created} onClose={() => setCreated(null)} /> : null}
    </View>
  )
}
