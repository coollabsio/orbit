import { toast } from 'sonner'
import type { PrAutomationRule } from '@/api/generated/types.gen'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import { usePrAutomation, useSetPrAutomation } from '@/features/tasks/api/projects'

const EVENTS: { event: string; label: string; description: string; fallback: string }[] = [
  { event: 'draft', label: 'Draft pull request', description: 'A draft is opened, or a pull request is converted to draft.', fallback: 'no change' },
  { event: 'open', label: 'Pull request opened', description: 'Opened, reopened, or made ready for review.', fallback: 'first started status' },
  { event: 'review', label: 'Review requested', description: 'A reviewer is requested.', fallback: 'no change' },
  { event: 'merged', label: 'Pull request merged', description: 'The last pull request that closes the task is merged.', fallback: 'first completed status' },
]

/** The selector value of a rule: `default`, `none`, or the id of a status. */
const valueOf = (rule: PrAutomationRule | undefined) => (rule?.mode === 'status' && rule.status_id ? rule.status_id : rule?.mode ?? 'default')

/**
 * The status that a task of the project takes when a linked pull request moves. Saves on change (no draft): each
 * selector is one decision.
 */
export function PrAutomationCard({ workspaceId, projectId, statuses }: { workspaceId: string; projectId: string; statuses: TaskStatusDef[] }) {
  const rules = usePrAutomation(workspaceId, projectId)
  const save = useSetPrAutomation(workspaceId, projectId)
  // the Duplicate status is entered only by marking a duplicate
  const selectable = statuses.filter((status) => status.category !== 'duplicate')

  return (
    <SettingsCard title="Pull request automation" description="The status that a task takes when a GitHub pull request that names it moves. A task that is done does not move.">
      {rules.isPending ? <p className="text-sm text-muted-foreground" role="status">Loading…</p> : null}
      {rules.isError ? <p className="text-sm text-muted-foreground" role="alert">The rules could not be loaded.</p> : null}
      {rules.data ? (
        <div className="flex flex-col divide-y divide-border">
          {EVENTS.map(({ event, label, description, fallback }) => {
            const options = [
              { value: 'default', label: `Default (${fallback})` },
              { value: 'none', label: 'No change' },
              ...selectable.map((status) => ({ value: status.id, label: status.name })),
            ]
            return (
              <div key={event} className="flex items-center justify-between gap-4 py-3 first:pt-0 last:pb-0 max-[599px]:flex-col max-[599px]:items-stretch">
                <div className="flex min-w-0 flex-col gap-0.5">
                  <span className="text-sm font-medium text-foreground">{label}</span>
                  <span className="text-xs text-muted-foreground">{description}</span>
                </div>
                <Select
                  items={options}
                  value={valueOf(rules.data.find((rule) => rule.event === event))}
                  onValueChange={(value) => {
                    const rule: PrAutomationRule = value === 'default' || value === 'none' ? { event, mode: value } : { event, mode: 'status', status_id: value as string }
                    save.mutate(rule, { onError: () => toast.error('Could not save the rule. Try again.') })
                  }}
                >
                  <SelectTrigger aria-label={label} className="w-56 shrink-0 max-[599px]:w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
                  </SelectContent>
                </Select>
              </div>
            )
          })}
        </div>
      ) : null}
    </SettingsCard>
  )
}
