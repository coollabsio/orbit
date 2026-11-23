/** The web's `ProviderLinks.tsx`: links to each provider's dashboard pages and guides, by the field they help fill. */
import * as WebBrowser from 'expo-web-browser'
import { Pressable, View } from 'react-native'

import { useTheme } from '../../../theme'
import { Icon, Text } from '../../../ui'

interface Link {
  label: string
  href: string
}

type Purpose = 'receive' | 'send'

const POSTSHIBA = { label: 'PostShiba', href: 'https://www.postshiba.com' }
const POSTSHIBA_SIGNATURES = { label: 'Webhook signatures', href: 'https://www.postshiba.com/docs/webhooks/verify-webhooks-requests' }
const MAILGUN = { label: 'Mailgun', href: 'https://app.mailgun.com' }
const MAILGUN_SIGNING = { label: 'Signing key guide', href: 'https://documentation.mailgun.com/docs/mailgun/user-manual/webhooks/securing-webhooks' }
const RESEND_KEYS = { label: 'API keys', href: 'https://resend.com/api-keys' }
const RESEND_WEBHOOKS = { label: 'Webhooks', href: 'https://resend.com/webhooks' }
const SES_CONSOLE = { label: 'SES console', href: 'https://console.aws.amazon.com/ses/home' }
const IAM_USERS = { label: 'IAM users', href: 'https://console.aws.amazon.com/iam/home#/users' }
const POSTMARK_SERVERS = { label: 'Postmark servers', href: 'https://account.postmarkapp.com/servers' }

/**
 * Where to get each provider's keys and set it up, for receiving and for sending, by the input they help fill.
 * The key `''` holds the links of the provider as a whole (no input of its own). `smtp` and `direct` have none.
 */
const LINKS: Record<string, Record<Purpose, Record<string, Link[]>>> = {
  postshiba: {
    receive: {
      api_key: [POSTSHIBA, { label: 'Receiving guide', href: 'https://www.postshiba.com/docs/dashboard/receiving/get-email-content' }],
      webhook_secret: [POSTSHIBA_SIGNATURES],
    },
    send: {
      host: [POSTSHIBA],
      webhook_secret: [POSTSHIBA_SIGNATURES],
    },
  },
  cloudflare: {
    receive: {
      '': [
        { label: 'Email Routing', href: 'https://dash.cloudflare.com/?to=/:account/:zone/email/routing' },
        { label: 'Email Workers guide', href: 'https://developers.cloudflare.com/email-routing/email-workers/' },
      ],
    },
    send: {
      '': [{ label: 'Email Service guide', href: 'https://developers.cloudflare.com/email-service/' }],
      api_token: [{ label: 'API tokens', href: 'https://dash.cloudflare.com/profile/api-tokens' }],
    },
  },
  postmark: {
    receive: {
      '': [POSTMARK_SERVERS, { label: 'Inbound guide', href: 'https://postmarkapp.com/developer/user-guide/inbound/configure-an-inbound-server' }],
    },
    send: {
      '': [{ label: 'SMTP guide', href: 'https://postmarkapp.com/developer/user-guide/send-email-with-smtp' }],
      server_token: [POSTMARK_SERVERS],
    },
  },
  mailgun: {
    receive: {
      '': [{ label: 'Receiving routes guide', href: 'https://documentation.mailgun.com/docs/mailgun/user-manual/receive-forward-store/receive-http' }],
      signing_key: [MAILGUN, MAILGUN_SIGNING],
    },
    send: {
      api_key: [MAILGUN],
      webhook_signing_key: [MAILGUN_SIGNING],
    },
  },
  resend: {
    receive: {
      api_key: [RESEND_KEYS],
      webhook_secret: [RESEND_WEBHOOKS, { label: 'Receiving guide', href: 'https://resend.com/docs/webhooks/emails/received' }],
    },
    send: {
      api_key: [RESEND_KEYS],
      webhook_secret: [RESEND_WEBHOOKS],
    },
  },
  ses: {
    receive: {
      '': [SES_CONSOLE, { label: 'Receiving guide', href: 'https://docs.aws.amazon.com/ses/latest/dg/receiving-email.html' }],
      access_key_id: [IAM_USERS],
    },
    send: {
      '': [SES_CONSOLE],
      access_key_id: [IAM_USERS],
      topic_arn: [{ label: 'Event publishing guide', href: 'https://docs.aws.amazon.com/ses/latest/dg/monitor-using-event-publishing.html' }],
    },
  },
}

/**
 * Links that open the provider's dashboard pages and guides in the browser: those that help fill `field` (put them
 * under its input), or without `field` those of the provider as a whole. Nothing when there are none.
 */
export function ProviderLinks({ kind, purpose, field = '' }: { kind: string; purpose: Purpose; field?: string }) {
  const { colors } = useTheme()
  const links = LINKS[kind]?.[purpose][field] ?? []
  if (links.length === 0) return null
  return (
    <View className="flex-row flex-wrap items-center gap-x-3 gap-y-1">
      {links.map((link) => (
        <Pressable key={link.href} accessibilityRole="link" accessibilityLabel={link.label} hitSlop={6} onPress={() => void WebBrowser.openBrowserAsync(link.href).catch(() => undefined)} className="flex-row items-center gap-0.5">
          <Text className="font-noto-medium text-[12px] text-primary">{link.label}</Text>
          <Icon name="arrowUpRight" size={12} color={colors.primary} />
        </Pressable>
      ))}
    </View>
  )
}
