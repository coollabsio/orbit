import { Link } from 'react-router'
import { mcpClientCommands, mcpUrl } from '@orbit/api/mcp'
import { SettingsCard } from '@/components/common/SettingsCard'
import { CopyCodeButton } from '@/components/common/CopyCodeButton'
import { OAuthConnections } from '@/features/auth/components/OAuthConnections'

/** Account view: connect AI clients to Orbit's read-only task MCP server, and see the connected ones. */
export function AgenticPage() {
  const url = mcpUrl(window.location.origin)
  return (
    <>
      <SettingsCard title="MCP server" description="AI clients such as Claude, Codex and Grok can read your tasks through this address. The client signs in with your Orbit account, and you select the workspace and the projects it can read.">
        <CodeBlock text={url} />
      </SettingsCard>
      <SettingsCard title="Connect a client" description="Run the command for your client in a terminal. Then sign in to Orbit in the browser, select the projects and click Allow read access.">
        <div className="flex flex-col gap-4">
          {mcpClientCommands(url).map((entry) => (
            <div key={entry.client} className="flex flex-col gap-1.5">
              <p className="text-[13px] font-medium text-foreground">{entry.client}</p>
              <CodeBlock text={entry.command} />
              <p className="text-xs text-muted-foreground">{entry.signIn}</p>
            </div>
          ))}
          <p className="text-xs text-muted-foreground">
            A client that cannot sign in with OAuth can send a read-only API token instead. A workspace admin creates it in{' '}
            <Link to="/settings/api-tokens" className="text-foreground underline underline-offset-2">Settings → API tokens</Link>.
          </p>
        </div>
      </SettingsCard>
      <OAuthConnections />
    </>
  )
}

function CodeBlock({ text }: { text: string }) {
  return (
    <div className="group relative rounded-md bg-muted/50 py-2.5 pr-20 pl-3">
      <pre className="font-mono text-xs leading-5 break-all whitespace-pre-wrap text-foreground">{text}</pre>
      <CopyCodeButton getText={() => text} className="absolute top-1.5 right-1.5" />
    </div>
  )
}
