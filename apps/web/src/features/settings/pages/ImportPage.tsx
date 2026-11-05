import { Link } from 'react-router'
import { Import } from 'reicon-react'
import { SettingsCard } from '@/components/common/SettingsCard'
import { buttonVariants } from '@/components/ui/button'
import { RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'

const SOURCES = [
  { name: 'Notion', description: 'Import pages and databases into Docs.', to: '/docs/import' },
  { name: 'Asana', description: 'Import projects and their tasks into Tasks.', to: '/tasks-import' },
]

/** Where to bring data into the workspace from other tools. */
export function ImportPage() {
  return (
    <SettingsCard title="Import" flush>
      {SOURCES.map((source) => (
        <SettingsRow key={source.name} className="flex-nowrap">
          <RowIcon>
            <Import />
          </RowIcon>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
            <span className="font-medium text-foreground">{source.name}</span>
            <span className="text-xs text-muted-foreground">{source.description}</span>
          </div>
          <Link to={source.to} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
            Import from {source.name}
          </Link>
        </SettingsRow>
      ))}
    </SettingsCard>
  )
}
