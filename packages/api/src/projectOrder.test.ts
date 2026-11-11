import { describe, expect, test } from 'bun:test'
import { moveProject, orderProjects } from './projectOrder'

const projects = ['alpha', 'beta', 'gamma', 'delta'].map((id) => ({ id }))

describe('orderProjects', () => {
  test('puts the placed projects first and keeps the others in their API order', () => {
    expect(orderProjects(projects, ['gamma', 'alpha']).map((p) => p.id)).toEqual(['gamma', 'alpha', 'beta', 'delta'])
  })

  test('ignores ids of projects that are gone', () => {
    expect(orderProjects(projects, ['gone', 'delta']).map((p) => p.id)).toEqual(['delta', 'alpha', 'beta', 'gamma'])
  })

  test('keeps the API order without a saved order', () => {
    expect(orderProjects(projects, []).map((p) => p.id)).toEqual(['alpha', 'beta', 'gamma', 'delta'])
  })
})

describe('moveProject', () => {
  const ids = projects.map((p) => p.id)

  test('moves a project down and up', () => {
    expect(moveProject(ids, 'alpha', 2)).toEqual(['beta', 'gamma', 'alpha', 'delta'])
    expect(moveProject(ids, 'delta', 0)).toEqual(['delta', 'alpha', 'beta', 'gamma'])
  })

  test('returns null when nothing moves', () => {
    expect(moveProject(ids, 'alpha', 0)).toBeNull()
    expect(moveProject(ids, 'alpha', -1)).toBeNull()
    expect(moveProject(ids, 'delta', 4)).toBeNull()
    expect(moveProject(ids, 'gone', 1)).toBeNull()
  })
})
