import { afterAll, afterEach, expect, spyOn, test } from 'bun:test'
import { toast } from 'sonner'
import { ApiProblem } from '@/api/problem'
import { BulkTaskLimitError } from '@/features/tasks/api/tasks'
import { reportMoveError } from './moveErrors'

const errorToast = spyOn(toast, 'error').mockImplementation(() => 0)
afterEach(() => errorToast.mockClear())
afterAll(() => errorToast.mockRestore())

const problem = (status: number, code: string) => new ApiProblem({
  type: 'about:blank', title: 'Failed', status, code, detail: 'Failed.', instance: '/tasks/bulk', request_id: 'request-1',
})

test('a version conflict shows no toast: the mutation already asks to refresh the task', () => {
  reportMoveError(problem(409, 'task_conflict'))
  expect(errorToast).not.toHaveBeenCalled()
})

test('a move over the bulk limit explains how to split it', () => {
  reportMoveError(new BulkTaskLimitError(140))
  expect(errorToast).toHaveBeenCalledWith('This move would update 140 tasks. Move it in smaller steps so each drop affects at most 100 tasks.')
})

test('any other move error shows a generic toast', () => {
  reportMoveError(problem(500, 'failed'))
  expect(errorToast).toHaveBeenCalledWith('Could not move the task.')
})
