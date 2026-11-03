import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { createEditorSchemas } from './schema'
import { schemaTable } from './schemaTable'

const SERVER_SCHEMA = join(import.meta.dir, '../../../apps/server/src/collab/blocknote-schema.json')

describe('createEditorSchemas', () => {
  test('any views give the schema the server converts with', () => {
    const { pageEditorSchema } = createEditorSchemas({
      page: { render: () => null },
      callout: { render: () => null },
      mention: { render: () => null },
      codeBlock: { tokenClass: () => undefined },
    })
    expect(JSON.parse(JSON.stringify(schemaTable(pageEditorSchema)))).toEqual(JSON.parse(readFileSync(SERVER_SCHEMA, 'utf8')))
  })
})
