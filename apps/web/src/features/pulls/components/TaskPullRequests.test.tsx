import { expect, test } from 'bun:test'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { TaskPullRequests } from './TaskPullRequests'

test('a GitHub pull request opens in Orbit, with a link to GitHub next to it', () => {
  const view = renderWithWorkspace(<TaskPullRequests links={[{ kind: 'pull_request', source: false, state: 'merged', title: 'Fix the login', url: 'https://github.com/acme/repo/pull/12' }]} />)
  expect(view.getByRole('link', { name: /Fix the login/ }).getAttribute('href')).toBe('/tasks/pulls/acme/repo/12')
  expect(view.getByText('Merged PR')).toBeTruthy()
  const github = view.getByRole('link', { name: 'Open on GitHub' })
  expect(github.getAttribute('href')).toBe('https://github.com/acme/repo/pull/12')
  expect(github.getAttribute('target')).toBe('_blank')
})

test('a link Orbit cannot read stays a plain GitHub link', () => {
  const view = renderWithWorkspace(<TaskPullRequests links={[{ kind: 'pull_request', source: false, state: 'open', title: 'Other', url: 'https://ghe.example/acme/repo/pull/1' }]} />)
  expect(view.getByRole('link', { name: /Other/ }).getAttribute('href')).toBe('https://ghe.example/acme/repo/pull/1')
})
