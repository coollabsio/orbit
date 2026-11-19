import { Link } from 'react-router'
import { ProviderMark } from '@/components/common/icons/ProviderMark'
import { Button, buttonVariants } from '@/components/ui/button'
import { useConnectGithub, useGithubConnection } from '@/features/github/api'
import { useCan } from '@/features/workspaces/permissions'

/** Shown in place of PR content when the member cannot use GitHub yet. Renders nothing when they are connected. */
export function ConnectGithubPrompt({ workspaceId }: { workspaceId: string }) {
  const connection = useGithubConnection(workspaceId)
  const connect = useConnectGithub(workspaceId)
  const canManage = useCan('integrations.manage')
  const status = connection.data?.status
  if (!status || status === 'connected') return null

  const askAdmin = (problem: string) => <div className="flex flex-col items-center gap-3 py-10 text-center">
    <p className="max-w-sm text-sm text-muted-foreground">{problem} {canManage ? '' : 'Ask a workspace owner or admin.'}</p>
    {canManage ? <Link className={buttonVariants({ variant: 'outline' })} to="/settings/github">Open GitHub settings</Link> : null}
  </div>

  if (status === 'needs_recreate') return askAdmin('To review pull requests in Orbit, re-create the GitHub App of this workspace.')
  if (status === 'unavailable') return askAdmin('This workspace has no GitHub App yet.')
  const broken = status === 'broken'
  return <div className="flex flex-col items-center gap-3 py-10 text-center">
    <span className="text-foreground [&_svg]:size-6"><ProviderMark provider="github" /></span>
    <p className="max-w-sm text-sm text-muted-foreground">{broken ? 'GitHub no longer accepts your connection. Connect again to see this pull request.' : 'Connect GitHub to see this pull request.'}</p>
    <Button disabled={connect.isPending} onClick={() => connect.mutate()}>{broken ? 'Reconnect GitHub' : 'Connect GitHub'}</Button>
    {connect.isError ? <p role="alert" className="text-xs text-destructive">The connection could not start. Try again.</p> : null}
  </div>
}
