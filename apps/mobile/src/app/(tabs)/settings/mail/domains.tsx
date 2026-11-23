import { useState } from 'react'
import { View } from 'react-native'

import { DomainCard } from '../../../../components/settings/mail/DomainCard'
import { MailManagerGate, useMailManager } from '../../../../components/settings/mail/DomainGate'
import { RouteSelect } from '../../../../components/settings/mail/DomainRoute'
import { Note, SettingsCard, SettingsShell, WORKSPACE_NAV } from '../../../../components/settings/Shell'
import { DIRECT, isMailDisabled, mailProblem } from '../../../../data/mail-settings'
import { useDomainWrites, useMailboxList, useMailDomains, useMailProviders, useMailSendRoutes } from '../../../../hooks/mail-domains'
import { Button, EmptyState, Input, LabelTip, Text } from '../../../../ui'

/** The web's Settings → Mail → Domains: add a domain, publish its records, check them, catch-all, turn mail off, remove. */
export default function MailDomainsScreen() {
  const manager = useMailManager()
  const domains = useMailDomains(manager)
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={manager ? () => domains.refetch() : undefined}>
      <MailManagerGate title="Mail domains">
        <MailDomains />
      </MailManagerGate>
    </SettingsShell>
  )
}

function MailDomains() {
  const domains = useMailDomains()
  const mailboxes = useMailboxList()
  const providerList = useMailProviders('workspace')
  const providers = providerList.data?.providers ?? []
  const sendRoutes = useMailSendRoutes('workspace')
  const { create } = useDomainWrites()
  const [name, setName] = useState('')
  const [route, setRoute] = useState(DIRECT)
  /** The domain added last: its card opens. */
  const [added, setAdded] = useState<{ id: string; name: string } | null>(null)
  /** The server said the instance switch is off; it stays shown until the page is opened again. */
  const [disabled, setDisabled] = useState(false)

  function submit() {
    const trimmed = name.trim()
    if (!trimmed || create.isPending) return
    create.mutate(
      { name: trimmed, providerId: route === DIRECT ? null : route },
      {
        onSuccess: (domain) => {
          setName('')
          setAdded({ id: domain.id, name: domain.name })
        },
        onError: (error) => {
          if (isMailDisabled(error)) setDisabled(true)
        },
      },
    )
  }

  return (
    <>
      {disabled ? (
        <View accessibilityRole="alert" className="gap-1 rounded-lg border border-border bg-card p-3">
          <Text className="font-noto-medium text-[13px] text-foreground">Mail is off on this Orbit server</Text>
          <Text className="text-[13px] leading-[19px] text-muted-foreground">An instance administrator can turn it on in Admin → Mail.</Text>
        </View>
      ) : null}
      <SettingsCard title="Add a domain" description="Orbit receives mail for the domain once its verification record is in the DNS.">
        <View className="gap-3">
          <Input
            label="Domain"
            help="A domain belongs to one workspace. A subdomain of a domain of another workspace is refused."
            placeholder="example.com"
            autoCapitalize="none"
            autoCorrect={false}
            autoComplete="off"
            keyboardType="url"
            returnKeyType="done"
            value={name}
            editable={!disabled}
            onChangeText={(value) => {
              setName(value)
              if (create.isError) create.reset()
            }}
            onSubmitEditing={submit}
          />
          {providers.length > 0 ? (
            <View className="gap-1.5">
              <LabelTip label="Receive mail" text="Through a provider, this server needs no open port 25. You can change it later." />
              <RouteSelect label="Where the domain's mail arrives" value={route} providers={providers} disabled={disabled} onChange={setRoute} />
            </View>
          ) : null}
          <View className="flex-row">
            <Button label={create.isPending ? 'Adding…' : 'Add domain'} disabled={!name.trim() || create.isPending || disabled} onPress={submit} />
          </View>
          {create.isError && !disabled ? <Note error>{mailProblem(create.error, 'The domain could not be added.')}</Note> : null}
          {added && !create.isError ? <Note>{`${added.name} added. Publish its records below.`}</Note> : null}
        </View>
      </SettingsCard>
      {domains.isPending ? (
        <SettingsCard title="Domains">
          <Note loading>Loading domains…</Note>
        </SettingsCard>
      ) : null}
      {domains.isError ? (
        <SettingsCard title="Domains">
          <View className="flex-row flex-wrap items-center gap-2">
            <Note error>Domains could not be loaded.</Note>
            <Button label="Retry" variant="ghost" size="sm" onPress={() => void domains.refetch()} />
          </View>
        </SettingsCard>
      ) : null}
      {domains.data?.length === 0 ? <EmptyState icon="global" title="No mail domains yet" description="Add a domain above, then publish its DNS records." /> : null}
      {domains.data?.map((domain) => (
        <DomainCard
          key={domain.id}
          listed={domain}
          mailboxes={mailboxes.data ?? []}
          providers={providers}
          sendRoutes={sendRoutes.data ?? null}
          defaultOpen={domain.id === added?.id || domains.data?.length === 1}
          justAdded={domain.id === added?.id}
        />
      ))}
    </>
  )
}
