import { Button } from '@/components/ui/button'
import { Dropdown } from '../../../components/ui/Dropdown'
import { PriorityIcon } from '../../../components/workspace/PriorityIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '../../../components/workspace/taskMeta'
import type { Task } from '../api/models'
import { useUpdateTask } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'

const MENU = 'flex min-w-[180px] flex-col gap-px p-1'
const OPTION =
  'group flex w-full min-h-8 items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-accent data-[selected]:bg-accent data-[selected]:font-medium'

/** Priority glyph that opens a menu to change the priority in place (list rows and board cards). */
export function PriorityPicker({ task, align = 'left' }: { task: Task; align?: 'left' | 'right' }) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  return (
    <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <Dropdown
        align={align}
        trigger={() => (
          <Button variant="ghost" size="icon-sm" className="size-[22px] text-muted-foreground/70" aria-label={`Priority: ${PRIORITY_LABEL[task.priority]}`} title="Change priority">
            <PriorityIcon priority={task.priority} />
          </Button>
        )}
      >
        {(close) => (
          <div className={MENU}>
            {PRIORITY_ORDER.map((priority) => (
              <button
                key={priority}
                className={OPTION}
                data-selected={priority === task.priority || undefined}
                onClick={() => {
                  updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, priority } })
                  close()
                }}
              >
                <PriorityIcon priority={priority} />
                {PRIORITY_LABEL[priority]}
              </button>
            ))}
          </div>
        )}
      </Dropdown>
      {updateTask.isError ? <span role="alert" className="text-xs text-destructive">Priority update failed. <Button variant="ghost" onClick={() => updateTask.variables && updateTask.mutate(updateTask.variables)}>Retry</Button></span> : null}
    </div>
  )
}
