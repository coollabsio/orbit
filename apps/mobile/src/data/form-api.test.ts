import { expect, test } from 'bun:test'

import { formScreenOf } from './form-api'

const server = 'https://orbit.example.com'

test('formScreenOf maps a web form link to the native form screen, keeping a linked view block', () => {
  expect(formScreenOf(`${server}/docs/db1/form/v1`, server)).toBe('/docs/form/db1/v1')
  expect(formScreenOf(`${server}/docs/db1/form/v1?link=l1`, server)).toBe('/docs/form/db1/v1?link=l1')
  expect(formScreenOf('https://elsewhere.example.com/docs/db1/form/v1', server)).toBeNull()
  expect(formScreenOf(`${server}/docs/db1`, server)).toBeNull()
})
