/** The web's `ProviderForm.tsx`: add an incoming provider, or change one (secrets left empty stay as they are). */
import { useState } from 'react'
import { View } from 'react-native'

import type { MailProviderKind, MailProviderSecretsBody, MailProviderSettings } from '@orbit/api/generated/types.gen'
import type { MailProviderView, ProviderScope } from '../../../data/mail-domains-api'
import { defaultMxHosts, mailProblem, PROVIDER_LABELS } from '../../../data/mail-settings'
import { useProviderWrites } from '../../../hooks/mail-domains'
import { Button, Input, LabelTip, Text } from '../../../ui'
import { Dialog } from '../Dialog'
import { FieldLabel, Note, Segmented, Select } from '../Shell'
import { ProviderLinks } from './ProviderLinks'

const RECOMMENDED: MailProviderKind = 'postshiba'
const KIND_OPTIONS = (Object.keys(PROVIDER_LABELS) as MailProviderKind[]).map((kind) => ({
  value: kind,
  label: kind === RECOMMENDED ? `${PROVIDER_LABELS[kind]} (recommended)` : PROVIDER_LABELS[kind],
}))

const MAILGUN_REGIONS = [
  { value: 'us', label: 'US' },
  { value: 'eu', label: 'EU' },
] as const

/** The secrets each kind needs from its provider; Orbit makes Cloudflare's and Postmark's. */
const SECRET_FIELDS: Record<MailProviderKind, { name: keyof MailProviderSecretsBody; label: string; hint?: string }[]> = {
  postshiba: [
    { name: 'api_key', label: 'API key', hint: 'Create it in PostShiba while signed in as a team admin: a key of another role cannot read received messages. Orbit only uses it to read the raw message of each received email.' },
    { name: 'webhook_secret', label: 'Inbox signing secret', hint: 'In PostShiba, open the inbox of the domain, then its Webhook dialog (webhook URL: the one Orbit shows after you add the provider; format: PostShiba JSON). Copy the signing secret from there.' },
  ],
  cloudflare: [],
  postmark: [],
  mailgun: [{ name: 'signing_key', label: 'HTTP webhook signing key', hint: 'Mailgun → Settings → API Security → HTTP webhook signing key. One key for the whole account; it is not the API key.' }],
  resend: [
    { name: 'api_key', label: 'API key', hint: 'Resend → API keys → Create, with permission Full access. A Sending access key cannot read received emails.' },
    { name: 'webhook_secret', label: 'Webhook signing secret', hint: 'Resend → Webhooks → add a webhook for the email.received event with the URL that Orbit shows after you add the provider. Its signing secret starts with whsec_.' },
  ],
  ses: [
    { name: 'access_key_id', label: 'Access key ID', hint: 'An access key of an IAM user whose policy allows s3:GetObject and s3:DeleteObject on the bucket (arn:aws:s3:::<bucket>/*). It needs no other permission.' },
    { name: 'secret_access_key', label: 'Secret access key' },
  ],
}

/** The web's tag input as one field: hosts separated by commas, spaces or new lines. */
const parseHosts = (text: string) => text.split(/[\s,]+/).filter((host) => host !== '')

const plain = { autoCapitalize: 'none', autoCorrect: false, autoComplete: 'off' } as const

export function ProviderForm({ scope, provider, onClose, onCreated }: { scope: ProviderScope; provider?: MailProviderView; onClose: () => void; onCreated?: (created: MailProviderView) => void }) {
  const { create, update } = useProviderWrites(scope)
  const [kind, setKind] = useState<MailProviderKind>(provider?.kind ?? RECOMMENDED)
  const [name, setName] = useState(provider?.name ?? '')
  const [secrets, setSecrets] = useState<MailProviderSecretsBody>({})
  const [settings, setSettings] = useState<MailProviderSettings>(provider?.settings ?? { mx_hosts: [] })
  const [mx, setMx] = useState((provider?.settings.mx_hosts ?? []).join(', '))
  const saving = create.isPending || update.isPending
  const error = create.error ?? update.error
  const fields = SECRET_FIELDS[kind]
  const secretsGiven = fields.every((field) => (secrets[field.name] ?? '').trim() !== '')
  const secretsTouched = fields.some((field) => (secrets[field.name] ?? '').trim() !== '')
  const setting = (key: 'region' | 'bucket' | 'prefix' | 'topic_arn', value: string) => setSettings((current) => ({ ...current, [key]: value.trim() === '' ? null : value }))
  const ready = provider ? !secretsTouched || secretsGiven : secretsGiven

  function submit() {
    if (saving || !ready) return
    const body = { name: name.trim() || PROVIDER_LABELS[kind], settings: { ...settings, mx_hosts: parseHosts(mx) } }
    if (provider) {
      update.mutate({ providerId: provider.id, body: { ...body, secrets: secretsTouched ? secrets : null } }, { onSuccess: onClose })
      return
    }
    create.mutate(
      { kind, ...body, secrets: fields.length > 0 ? secrets : null },
      {
        onSuccess: (created) => {
          onCreated?.(created)
          onClose()
        },
      },
    )
  }

  return (
    <Dialog
      visible
      title={provider ? `Change ${provider.name}` : 'Add an incoming provider'}
      description="Incoming mail: the provider receives the domain's mail and sends each message to Orbit over HTTPS: this server needs no open port 25."
      onClose={saving ? () => undefined : onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" disabled={saving} onPress={onClose} />
          <Button label={saving ? 'Saving…' : provider ? 'Save' : 'Add provider'} disabled={saving || !ready} onPress={submit} />
        </>
      }
    >
      <View className="gap-4">
        <View className="gap-1.5">
          <FieldLabel>Provider</FieldLabel>
          <Select
            label="Provider"
            value={kind}
            options={KIND_OPTIONS}
            disabled={Boolean(provider)}
            onChange={(value) => {
              setKind(value)
              setSecrets({})
            }}
          />
        </View>
        <Input label="Name" placeholder={PROVIDER_LABELS[kind]} value={name} onChangeText={setName} {...plain} />
        <ProviderLinks kind={kind} purpose="receive" />
        {fields.length === 0 && !provider ? <Text className="text-[13px] leading-[19px] text-muted-foreground">Orbit makes the secret for this provider and shows it once, after you add it.</Text> : null}
        {fields.map((field) => (
          <View key={field.name} className="gap-1.5">
            <Input
              label={field.label}
              help={field.hint}
              secureTextEntry
              placeholder={provider?.secrets_set.includes(field.name) ? 'Set. Leave empty to keep it.' : undefined}
              value={secrets[field.name] ?? ''}
              onChangeText={(value) => setSecrets((current) => ({ ...current, [field.name]: value }))}
              {...plain}
            />
            <ProviderLinks kind={kind} purpose="receive" field={field.name} />
          </View>
        ))}
        {kind === 'mailgun' ? (
          <View className="gap-1.5">
            <FieldLabel>Region</FieldLabel>
            <Segmented label="Region" value={settings.region === 'eu' ? 'eu' : 'us'} options={MAILGUN_REGIONS} onChange={(value) => setting('region', value === 'eu' ? 'eu' : '')} />
          </View>
        ) : null}
        {kind === 'ses' ? (
          <>
            <Input label="AWS region" placeholder="us-east-1" value={settings.region ?? ''} onChangeText={(value) => setting('region', value)} {...plain} />
            <Input label="S3 bucket" value={settings.bucket ?? ''} onChangeText={(value) => setting('bucket', value)} {...plain} />
            <Input label="Key prefix" placeholder="Optional" value={settings.prefix ?? ''} onChangeText={(value) => setting('prefix', value)} {...plain} />
            <Input label="SNS topic ARN" placeholder="arn:aws:sns:…" value={settings.topic_arn ?? ''} onChangeText={(value) => setting('topic_arn', value)} {...plain} />
          </>
        ) : null}
        <View className="gap-1.5">
          <LabelTip
            label="MX hosts"
            text="The provider’s mail server hostnames, not your domains. Domains routed to this provider must point their MX record to these hosts. Leave empty to use the provider’s usual hosts. Separate hosts with commas."
          />
          <Input
            accessibilityLabel="MX hosts"
            placeholder={kind === 'resend' ? 'The MX host that Resend shows' : `Empty: ${defaultMxHosts(kind, settings.region).join(', ')}`}
            value={mx}
            onChangeText={setMx}
            keyboardType="url"
            {...plain}
          />
        </View>
        {error ? <Note error>{mailProblem(error, 'The provider was not saved.')}</Note> : null}
      </View>
    </Dialog>
  )
}
