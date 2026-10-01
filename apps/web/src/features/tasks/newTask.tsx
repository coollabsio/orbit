import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import type { CreateTaskBody, TaskRecord } from '@/api/generated/types.gen'
import { NewTaskDialog } from '@/features/tasks/components/NewTaskDialog'
import type { GroupValues } from '@/features/views/layoutGroups'
import { useCommand } from '@/shortcuts/useCommand'

/** What the current page knows about a new task: its starting properties and where a created task opens. */
export interface NewTaskSource {
  /** `values` = the group (and sub-group) whose + was pressed; empty from the keyboard or the top bar. */
  defaults: (values: GroupValues) => Partial<CreateTaskBody>
  onOpenTask?: (task: TaskRecord) => void
}

interface NewTaskRequest {
  values?: GroupValues
}

const NewTaskContext = createContext<{ open: (request?: NewTaskRequest) => void; register: (source: NewTaskSource) => () => void }>({ open: () => {}, register: () => () => {} })

/** Owns the one new task dialog of the app. `C` opens it on every page; the page in view supplies the defaults. */
export function NewTaskProvider({ children }: { children: ReactNode }) {
  const navigate = useNavigate()
  const source = useRef<NewTaskSource | null>(null)
  const [dialog, setDialog] = useState<{ defaults: Partial<CreateTaskBody>; onOpenTask: (task: TaskRecord) => void } | null>(null)

  const open = useCallback((request: NewTaskRequest = {}) => {
    const page = source.current
    setDialog((current) => current ?? {
      defaults: page?.defaults(request.values ?? []) ?? {},
      onOpenTask: page?.onOpenTask ?? ((task) => navigate(`/tasks/${task.id}`)),
    })
  }, [navigate])
  const register = useCallback((page: NewTaskSource) => {
    source.current = page
    return () => {
      if (source.current === page) source.current = null
    }
  }, [])
  useCommand('task.create', () => open())

  return (
    <NewTaskContext value={{ open, register }}>
      {children}
      {dialog ? <NewTaskDialog defaults={dialog.defaults} onClose={() => setDialog(null)} onOpenTask={dialog.onOpenTask} /> : null}
    </NewTaskContext>
  )
}

/** Opens the new task dialog, e.g. from a "New task" button or the + of a group. */
export const useOpenNewTask = () => useContext(NewTaskContext).open

/** Makes the calling page the source of new task defaults while it is mounted. */
export function useCreateTaskDefaults(source: NewTaskSource) {
  const { register } = useContext(NewTaskContext)
  useEffect(() => register(source))
}
