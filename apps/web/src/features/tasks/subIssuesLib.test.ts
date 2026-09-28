import { expect, test } from 'bun:test'
import { ApiProblem } from '@/api/problem'
import { parentErrorMessage, parentPickerTitle, parentToastMessage } from './subIssuesLib'

const problem = (code: string) => new ApiProblem({ type: 'about:blank', title: 'Unprocessable', status: 422, detail: 'x', code, instance: '/', request_id: 'r' } as never)

test('parent toasts name the task and the new parent, or say it is no longer a sub-issue', () => {
  expect(parentToastMessage(['ORB-31'], 'ORB-12')).toBe('ORB-31 is now a sub-issue of ORB-12')
  expect(parentToastMessage(['ORB-31', 'ORB-32'], 'ORB-12')).toBe('2 tasks are now sub-issues of ORB-12')
  expect(parentToastMessage(['ORB-31'], null)).toBe('ORB-31 is no longer a sub-issue')
  expect(parentToastMessage(['ORB-31', 'ORB-32'], null)).toBe('2 tasks are no longer sub-issues')
})

test('server refusals explain themselves', () => {
  expect(parentErrorMessage(problem('parent_cycle'))).toBe('A task can’t be a sub-issue of itself or of its own sub-issues.')
  expect(parentErrorMessage(problem('parent_invalid'))).toBe('That parent task isn’t available anymore.')
  expect(parentErrorMessage(new Error('boom'))).toBe('Couldn’t change the parent.')
})

test('the parent picker title names the task or the count', () => {
  expect(parentPickerTitle('ORB-31')).toBe('Set parent of ORB-31…')
  expect(parentPickerTitle(3)).toBe('Set parent of 3 tasks…')
})
