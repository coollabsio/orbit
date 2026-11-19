import type { GithubPullDetail } from '@orbit/api/generated/types.gen'
import type { ReviewSummary } from '@orbit/api/github'
import { openURL } from 'expo-linking'
import { type ReactNode, useState } from 'react'
import { Pressable, View } from 'react-native'

import { Text } from '../../ui'

const DOT: Record<string, string> = { success: 'bg-emerald-500', failure: 'bg-red-500', pending: 'bg-amber-500', neutral: 'bg-muted-foreground' }

const names = (logins: string[]) => (logins.length <= 2 ? logins.join(' and ') : `${logins[0]} and ${logins.length - 1} more`)

function Row({ label, first = false, children }: { label: string; first?: boolean; children: ReactNode }) {
  return (
    <View className={`min-h-9 flex-row items-center gap-3 border-border px-3 py-1.5 ${first ? '' : 'border-t'}`}>
      <Text className="w-16 text-[12px] text-muted-foreground">{label}</Text>
      <View className="min-w-0 flex-1 flex-row flex-wrap items-center gap-x-2 gap-y-1">{children}</View>
    </View>
  )
}

/** The web's `PullSummary`: what to read, who decided, what waits for the member, and CI, in four short rows. A tap
 *  on the open threads opens the next one; a tap on the checks lists them (the web's popover). */
export function PullSummary({ pull, summary, onNextThread }: { pull: GithubPullDetail; summary: ReviewSummary; onNextThread: () => void }) {
  const [checksOpen, setChecksOpen] = useState(false)
  const runs = pull.checks.runs
  const failed = runs.filter((run) => run.state === 'failure').length
  const running = runs.filter((run) => run.state === 'pending').length
  const checkText = runs.length === 0 ? 'No checks' : failed > 0 ? `${failed} of ${runs.length} failed` : running > 0 ? `${running} of ${runs.length} running` : `${runs.length} passed`
  return (
    <View accessibilityLabel="Summary" className="overflow-hidden rounded-lg border border-border">
      <Row label="Files" first>
        <Text className="text-[12px]">{`${summary.viewed} of ${summary.toReview} viewed`}</Text>
        {summary.aside > 0 ? <Text className="text-[12px] text-muted-foreground">{`${summary.aside} set aside`}</Text> : null}
      </Row>
      <Row label="Reviews">
        {summary.changesRequested.length > 0 ? (
          <Text numberOfLines={1} className="text-[12px] text-red-600 dark:text-red-400">{`Changes requested by ${names(summary.changesRequested)}`}</Text>
        ) : summary.approved.length > 0 ? (
          <Text numberOfLines={1} className="text-[12px] text-emerald-700 dark:text-emerald-400">{`Approved by ${names(summary.approved)}`}</Text>
        ) : (
          <Text className="text-[12px] text-muted-foreground">None yet</Text>
        )}
      </Row>
      <Row label="Threads">
        {summary.openThreads === 0 ? (
          <Text className="text-[12px] text-muted-foreground">{summary.resolvedThreads > 0 ? `All ${summary.resolvedThreads} resolved` : 'None'}</Text>
        ) : (
          <>
            <Pressable accessibilityRole="button" accessibilityLabel={`${summary.openThreads} open, open the next`} hitSlop={8} onPress={onNextThread} className="active:opacity-60">
              <Text className="text-[12px] underline">{`${summary.openThreads} open`}</Text>
            </Pressable>
            {summary.needYou > 0 ? (
              <View className="rounded-full bg-primary px-1.5 py-px">
                <Text className="font-noto-medium text-[11px] text-primary-foreground">{`${summary.needYou} for you`}</Text>
              </View>
            ) : null}
          </>
        )}
      </Row>
      <Row label="Checks">
        {runs.length === 0 ? (
          <Text className="text-[12px] text-muted-foreground">{checkText}</Text>
        ) : (
          <Pressable accessibilityRole="button" accessibilityState={{ expanded: checksOpen }} hitSlop={8} onPress={() => setChecksOpen(!checksOpen)} className="flex-row items-center gap-1.5 active:opacity-60">
            <View className={`size-2 rounded-full ${DOT[pull.checks.state] ?? DOT.neutral}`} />
            <Text className="text-[12px]">{checkText}</Text>
          </Pressable>
        )}
      </Row>
      {checksOpen
        ? runs.map((run) => (
            <Pressable key={run.name} accessibilityRole="link" accessibilityLabel={run.name} onPress={() => void openURL(run.url ?? pull.html_url)} className="min-h-9 flex-row items-center gap-2 border-t border-border py-1.5 pl-[88px] pr-3 active:bg-muted">
              <View className={`size-2 rounded-full ${DOT[run.state] ?? DOT.neutral}`} />
              <Text numberOfLines={1} className="flex-1 text-[12px]">
                {run.name}
              </Text>
            </Pressable>
          ))
        : null}
    </View>
  )
}
