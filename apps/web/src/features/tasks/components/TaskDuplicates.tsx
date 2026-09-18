import { useState } from 'react'
import { createPortal } from 'react-dom'
import { Link } from 'react-router'
import { Copy } from 'reicon-react'
import { TaskMentionSearch } from '../../../components/editor/TaskMentionSearch'
import { confirmAction } from '../../../components/ui/confirmAction'
import { Modal } from '../../../components/ui/Modal'
import { useWorkspace } from '../../workspaces/workspaceContext'
import type { Task } from '../api/models'
import { useMarkDuplicate, useTask, useUnmarkDuplicate } from '../api/tasks'

/**
 * A task's identifier and title from the loaded page, or — when it is not on the
 * page (the list hides sub-issues, filters, trash) — from its own detail query.
 */
function useTaskLabel(taskId: string, tasks: Task[]) {
  const { workspace } = useWorkspace()
  const onPage = tasks.find((task) => task.id === taskId)
  const fetched = useTask(workspace.id, onPage ? undefined : taskId)
  if (onPage) return { identifier: onPage.identifier, title: onPage.title }
  if (fetched.data) return { identifier: fetched.data.identifier, title: fetched.data.title }
  return undefined
}

/** Muted banner at the top of a duplicate's detail. Nothing moved; both stay readable. */
export function DuplicateBanner({ task, tasks }: { task: Task; tasks: Task[] }) {
  if (!task.duplicateOfTaskId) return null
  return <DuplicateBannerLink canonicalId={task.duplicateOfTaskId} tasks={tasks} />
}

function DuplicateBannerLink({ canonicalId, tasks }: { canonicalId: string; tasks: Task[] }) {
  const canonical = useTaskLabel(canonicalId, tasks)
  return (
    <div className="tasks-duplicate-banner" role="note">
      <Copy size={14} aria-hidden="true" />
      <Link to={`/tasks/${canonicalId}`}>Duplicate of {canonical?.identifier ?? 'another task'}</Link>
      {canonical ? <span className="tasks-duplicate-banner-title truncate">{canonical.title}</span> : null}
    </div>
  )
}

/** Sidebar group on the canonical task: the tasks marked as its duplicates. */
export function DuplicatesGroup({ task, tasks, onOpen }: { task: Task; tasks: Task[]; onOpen: (taskId: string) => void }) {
  if (task.duplicateIds.length === 0) return null
  return (
    <div className="tasks-side-group">
      <h4 className="tasks-side-heading">Duplicates</h4>
      {task.duplicateIds.map((id) => <DuplicateLink key={id} taskId={id} tasks={tasks} onOpen={onOpen} />)}
    </div>
  )
}

function DuplicateLink({ taskId, tasks, onOpen }: { taskId: string; tasks: Task[]; onOpen: (taskId: string) => void }) {
  const duplicate = useTaskLabel(taskId, tasks)
  return (
    <button
      type="button"
      className="button button-ghost tasks-side-prop tasks-side-link"
      aria-label={duplicate?.identifier ?? 'Duplicate task'}
      title={duplicate ? `${duplicate.identifier} · ${duplicate.title}` : undefined}
      onClick={() => onOpen(taskId)}
    >
      <Copy size={14} aria-hidden="true" />
      <span className="tasks-side-link-id">{duplicate?.identifier ?? '…'}</span>
      {duplicate ? <span className="tasks-side-link-title truncate">{duplicate.title}</span> : null}
    </button>
  )
}

/**
 * The whole "mark as duplicate" flow for one task. The mutations live here —
 * in TaskDetail, not in the dropdown or the picker — so a failure still has
 * somewhere to render after both have closed.
 */
export function useDuplicateFlow(task: Task | undefined) {
  const { workspace } = useWorkspace()
  const mark = useMarkDuplicate(workspace.id)
  const unmark = useUnmarkDuplicate(workspace.id)
  const [picking, setPicking] = useState(false)

  const pick = async (targetTaskId: string) => {
    setPicking(false)
    if (!task) return
    // Reversible, but it changes the status — one light confirm.
    const confirmed = await confirmAction({
      title: `Mark ${task.identifier} as a duplicate?`,
      description: 'It moves to Cancelled and stays fully readable. You can remove the mark from the same menu; that does not restore the previous status.',
      confirmLabel: 'Mark as duplicate',
    })
    if (!confirmed) return
    mark.mutate({ taskId: task.id, targetTaskId, version: task.version })
  }

  return {
    picking,
    openPicker: () => setPicking(true),
    closePicker: () => setPicking(false),
    pick,
    unmark: () => {
      if (task) unmark.mutate({ taskId: task.id })
    },
    mark,
    unmarkMutation: unmark,
  }
}

export type DuplicateFlow = ReturnType<typeof useDuplicateFlow>

/** Entries for the task detail's `⋯` menu. Never "Merge": nothing is merged. */
export function DuplicateMenuItems({ task, flow, close }: { task: Task; flow: DuplicateFlow; close: () => void }) {
  if (task.duplicateOfTaskId) {
    return (
      <button
        type="button"
        className="popover-option"
        onClick={() => {
          close()
          flow.unmark()
        }}
      >
        <Copy size={15} />
        Remove duplicate mark
      </button>
    )
  }
  return (
    <button
      type="button"
      className="popover-option"
      onClick={() => {
        close()
        flow.openPicker()
      }}
    >
      <Copy size={15} />
      Mark as duplicate…
    </button>
  )
}

/** The target picker. Reuses the `@` menu's search; there is no second search implementation. */
export function MarkDuplicateDialog({ task, flow, workspaceId }: { task: Task; flow: DuplicateFlow; workspaceId: string }) {
  if (!flow.picking) return null
  return createPortal(
    <Modal
      title={`Mark ${task.identifier} as a duplicate of…`}
      description="Pick the canonical task. Both tasks stay readable."
      onClose={flow.closePicker}
      maxWidth={480}
    >
      <TaskMentionSearch workspaceId={workspaceId} excludeTaskId={task.id} onPick={(taskId) => void flow.pick(taskId)} />
    </Modal>,
    document.body,
  )
}

/** Failure alerts for both directions, with a retry that repeats the same request. */
export function DuplicateErrors({ flow }: { flow: DuplicateFlow }) {
  return (
    <>
      {flow.mark.isError ? (
        <p role="alert" className="text-danger text-xs">
          Marking as duplicate failed.{' '}
          <button type="button" className="button button-ghost" onClick={() => flow.mark.variables && flow.mark.mutate(flow.mark.variables)}>Retry</button>
        </p>
      ) : null}
      {flow.unmarkMutation.isError ? (
        <p role="alert" className="text-danger text-xs">
          Removing the duplicate mark failed.{' '}
          <button type="button" className="button button-ghost" onClick={flow.unmark}>Retry</button>
        </p>
      ) : null}
    </>
  )
}
