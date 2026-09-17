import { Dropdown } from '../../../components/ui/Dropdown'
import { PriorityIcon } from '../../../components/workspace/PriorityIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '../../../components/workspace/taskMeta'
import type { Task } from '../api/models'
import { useUpdateTask } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'

/** Priority glyph that opens a menu to change the priority in place (list rows and board cards). */
export function PriorityPicker({ task, align = 'left' }: { task: Task; align?: 'left' | 'right' }) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  return (
    <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <Dropdown
        align={align}
        trigger={() => (
          <button className="icon-button tasks-inline-picker" aria-label={`Priority: ${PRIORITY_LABEL[task.priority]}`} title="Change priority">
            <PriorityIcon priority={task.priority} />
          </button>
        )}
      >
        {(close) => (
          <>
            {PRIORITY_ORDER.map((priority) => (
              <button
                key={priority}
                className="popover-option"
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
          </>
        )}
      </Dropdown>
      {updateTask.isError ? <span role="alert" className="text-danger text-xs">Priority update failed. <button className="button button-ghost" onClick={() => updateTask.variables && updateTask.mutate(updateTask.variables)}>Retry</button></span> : null}
    </div>
  )
}
