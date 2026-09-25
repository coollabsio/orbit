import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import { switchWorkspaceHref } from '@/features/workspaces/navigation'
import type { SavedView } from './api/views'

export function viewPath(viewId: string): string {
  return `/views/${viewId}`
}

/** Copies the view's URL with its workspace, so the link opens in the right workspace. */
export async function copyViewLink(workspaceId: string, viewId: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(`${window.location.origin}${switchWorkspaceHref(viewPath(viewId), '', workspaceId)}`)
    toast('Link copied')
  } catch {
    toast.error("Orbit couldn't copy the link.")
  }
}

export function confirmDeleteView(view: Pick<SavedView, 'name' | 'visibility'>): Promise<boolean> {
  return confirmAction({
    title: 'Delete view?',
    description: view.visibility === 'workspace' ? `“${view.name}” will be deleted for everyone in the workspace.` : `“${view.name}” will be deleted.`,
    confirmLabel: 'Delete view',
    danger: true,
  })
}
