import { describe, expect, test } from 'bun:test'
import filterCases from '../cases/filter.json'
import sortCases from '../cases/sort.json'
import { matchesFilter } from './filter'
import { sortRows } from './sort'
import type {
  DatabaseFilterGroup,
  DatabaseProperty,
  DatabaseRelatedPage,
  DatabaseRow,
  DatabaseViewSort,
  ViewContext,
} from './types'
import { parseProperty, parseRow } from './values'

type Fixture = {
  ctx: { user_id: string; now: string; time_zone: string; locale: string; members: Record<string, string> }
  related_pages?: Record<string, DatabaseRelatedPage>
  properties: unknown[]
  rows: unknown[]
}

function load(fixture: Fixture) {
  const properties = (fixture.properties as DatabaseProperty[]).map(parseProperty)
  const rows = (fixture.rows as DatabaseRow[]).map((row) => parseRow(row, properties))
  const ctx: ViewContext = {
    userId: fixture.ctx.user_id,
    now: Date.parse(fixture.ctx.now),
    timeZone: fixture.ctx.time_zone,
    locale: fixture.ctx.locale,
    members: new Map(Object.entries(fixture.ctx.members)),
    relatedPages: fixture.related_pages,
  }
  return { properties, rows, ctx }
}

describe('filter cases shared with the server (cases/filter.json)', () => {
  const { properties, rows, ctx } = load(filterCases as Fixture)
  const byId = new Map(properties.map((property) => [property.id, property]))
  test.each(filterCases.cases.map((item) => [item.name, item] as const))('%s', (_, item) => {
    const filter = item.filter as DatabaseFilterGroup
    const ids = rows.filter((row) => matchesFilter(row, filter, byId, ctx)).map((row) => row.id)
    expect(ids).toEqual(item.expected_ids)
  })
})

describe('sort cases shared with the server (cases/sort.json)', () => {
  const { properties, rows, ctx } = load(sortCases as Fixture)
  test.each(sortCases.cases.map((item) => [item.name, item] as const))('%s', (_, item) => {
    const ids = sortRows(rows, item.sorts as DatabaseViewSort[], properties, ctx).map((row) => row.id)
    expect(ids).toEqual(item.expected_ids)
  })
})
