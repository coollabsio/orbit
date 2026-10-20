import { apiClient } from '@/api/client'
import { uploadTaskAttachments } from '@/api/generated/sdk.gen'
import { taskAttachmentUrl } from '@/features/docs/editor/content'
import { shrinkImage } from '@/lib/shrinkImage'

/**
 * Uploads a file placed in a task's description or a comment (paste, drop, slash menu, paperclip) as an inline
 * attachment, which the attachment list leaves out, and resolves to the URL its image/file block shows.
 */
export async function uploadInlineFile(workspaceId: string, taskId: string, file: File): Promise<string> {
  const { data } = await uploadTaskAttachments({
    client: apiClient,
    path: { workspace_id: workspaceId, task_id: taskId },
    query: { inline: '1' },
    body: { file: await shrinkImage(file) },
    throwOnError: true,
  })
  return taskAttachmentUrl(workspaceId, taskId, data.id)
}
