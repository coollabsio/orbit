import { ago } from '@/lib/format'
import type { Webhook } from '@/mock/types'

export const webhooks: Webhook[] = [
  {
    id: 'wh_deploy',
    name: 'Deploy Bot',
    channelId: 'c_infra',
    token: 'a8f3c1e2d9b74f60b5e1',
    createdAt: ago(12, 'd'),
    iconUrl: null,
  },
  {
    id: 'wh_releases',
    name: 'Release Notes',
    channelId: 'c_engineering',
    token: '5c0d7e2a1b9f4c83e6d2',
    createdAt: ago(3, 'd'),
    iconUrl: null,
  },
]
