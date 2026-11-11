import { Lock, People } from 'reicon-react'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

/** Workspace or Private, as the new channel dialog's Visibility choice. Shared by the new project dialog and project settings. */
export function ProjectVisibilityChoice({ isPrivate, disabled, onChange }: { isPrivate: boolean; disabled?: boolean; onChange: (isPrivate: boolean) => void }) {
  return (
    <ToggleGroup
      variant="outline"
      orientation="vertical"
      className="w-full"
      disabled={disabled}
      value={[isPrivate ? 'private' : 'workspace']}
      onValueChange={(value) => {
        // a press on the chosen one must not leave the project without a choice
        const next = value.at(-1)
        if (next === 'workspace' || next === 'private') onChange(next === 'private')
      }}
    >
      <ToggleGroupItem value="workspace" className="h-auto flex-col items-start gap-0.5 py-2 whitespace-normal">
        <span className="flex items-center gap-1.5">
          <People aria-hidden />
          Workspace
        </span>
        <span className="text-xs font-normal text-muted-foreground">Every member of the workspace sees it.</span>
      </ToggleGroupItem>
      <ToggleGroupItem value="private" className="h-auto flex-col items-start gap-0.5 py-2 whitespace-normal">
        <span className="flex items-center gap-1.5">
          <Lock aria-hidden />
          Private
        </span>
        <span className="text-xs font-normal text-muted-foreground">Only the people you add, and workspace owners and admins.</span>
      </ToggleGroupItem>
    </ToggleGroup>
  )
}
