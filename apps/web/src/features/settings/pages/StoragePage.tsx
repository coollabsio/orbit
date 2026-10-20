import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { FieldGrid, SettingsRow } from '@/features/settings/components/SettingsParts'
import { formatBytes } from '@/lib/formatBytes'
import { cacheUsage, clearLocalCache, MEDIA_LIMITS_MB, useMediaLimit } from '@/lib/localCache'

const LIMIT_OPTIONS = MEDIA_LIMITS_MB.map((megabytes) => ({ value: String(megabytes), label: megabytes >= 1000 ? `${megabytes / 1000} GB` : `${megabytes} MB` }))
const USAGE_KEY = ['local-cache-usage'] as const

/** Account view: what Orbit keeps on this device, how much it may keep, and the button that removes it. */
export function StoragePage() {
  const queryClient = useQueryClient()
  const [limit, setLimit] = useMediaLimit()
  const usage = useQuery({ queryKey: USAGE_KEY, queryFn: cacheUsage, staleTime: 0, gcTime: 0 })
  const clear = useMutation({
    mutationFn: clearLocalCache,
    onSuccess: () => toast.success('The cache on this device is clear.'),
    onError: () => toast.error('The cache could not be cleared.'),
    onSettled: () => queryClient.invalidateQueries({ queryKey: USAGE_KEY }),
  })

  return (
    <SettingsCard
      title="Storage on this device"
      description="Orbit keeps a copy of your chat on this device, so a reload does not read everything from the server again. Sign-out removes it."
      actions={
        <Button type="button" variant="outline" disabled={clear.isPending} onClick={() => clear.mutate()}>
          Clear cache
        </Button>
      }
      flush
    >
      <div className="flex flex-col divide-y">
        <SettingsRow className="justify-between">
          <span className="text-[13px] font-medium text-foreground">Messages and conversations</span>
          <span className="text-xs text-muted-foreground tabular-nums">{usage.data ? `${usage.data.entries} ${usage.data.entries === 1 ? 'list' : 'lists'} · ${formatBytes(usage.data.entryBytes)}` : '…'}</span>
        </SettingsRow>
        <SettingsRow className="justify-between">
          <span className="text-[13px] font-medium text-foreground">Images</span>
          <span className="text-xs text-muted-foreground tabular-nums">
            {usage.data ? `${usage.data.mediaFiles} ${usage.data.mediaFiles === 1 ? 'file' : 'files'} · ${formatBytes(usage.data.mediaBytes)} of ${formatBytes(limit * 1_000_000)}` : '…'}
          </span>
        </SettingsRow>
        <div className="p-4">
          <FieldGrid>
            <Field>
              <FieldLabel htmlFor="media-limit">Space for images</FieldLabel>
              <Select
                items={LIMIT_OPTIONS}
                value={String(limit)}
                onValueChange={(value) => {
                  setLimit(Number(value))
                  void queryClient.invalidateQueries({ queryKey: USAGE_KEY })
                }}
              >
                <SelectTrigger id="media-limit">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {LIMIT_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
                </SelectContent>
              </Select>
              <FieldDescription>Over the limit, the images kept for the longest time go first. Messages are kept for the 100 conversations you opened last, for 30 days.</FieldDescription>
            </Field>
          </FieldGrid>
        </div>
      </div>
    </SettingsCard>
  )
}
