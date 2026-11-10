import { useId, useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Pane } from '@/components/common/Pane'
import { usePageTree } from '@/features/docs/api/pages'
import { useTeamspaces } from '@/features/docs/api/teamspaces'
import { ImportDestinationFields } from '@/features/docs/components/ImportDestinationFields'
import { ImportBody, ImportHeader } from '@/features/docs/components/ImportParts'
import { CsvImportFlow } from '@/features/docs/database/CsvImport'
import { importSummary } from '@/features/docs/database/csvImportText'
import { useImportDestination } from '@/features/docs/import/destination'

/** `/docs/import/csv`: a CSV becomes a new database (spec P2 9.4), placed like a Docs import. */
export function CsvImportPane({ workspaceId }: { workspaceId: string }) {
  const navigate = useNavigate()
  const titleId = useId()
  const teamspaces = useTeamspaces(workspaceId)
  const pages = usePageTree(workspaceId)
  const destination = useImportDestination(teamspaces.data, pages.data)
  const [title, setTitle] = useState('')
  const request = () => {
    const place = 'parent_page_id' in destination.body ? { parent_id: destination.body.parent_page_id } : destination.body
    return { title: title.trim(), ...place }
  }
  return (
    <Pane>
      <ImportHeader title="Import CSV" />
      <ImportBody>
        <section className="flex flex-col gap-4 rounded-lg border border-border p-5">
          <div className="flex flex-col gap-2">
            <h2 className="text-sm font-semibold text-foreground">Create a database from a CSV file</h2>
            <p className="text-sm text-muted-foreground">
              Each row becomes a row of the database, and you choose a property type for each column. One column holds the
              row titles. Values that do not fit their type are left empty; you see how many before the import.
            </p>
          </div>
          <CsvImportFlow
            workspaceId={workspaceId}
            destination={{ kind: 'new', request }}
            fields={
              <div className="flex flex-wrap gap-3">
                <div className="grid gap-1.5">
                  <Label htmlFor={titleId} className="text-xs text-muted-foreground">
                    Database name
                  </Label>
                  <Input id={titleId} className="w-56" placeholder="Untitled" value={title} maxLength={500} onChange={(event) => setTitle(event.target.value)} />
                </div>
                <ImportDestinationFields destination={destination} />
              </div>
            }
            onImported={(result, database) => {
              toast.success(importSummary(result))
              if (database) navigate(`/docs/${database.id}`)
            }}
          />
        </section>
      </ImportBody>
    </Pane>
  )
}
