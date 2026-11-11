import { createClient, defaultPlugins } from '@hey-api/openapi-ts'
import { mkdir, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const [input = '../../packages/api/src/generated/openapi.json', output = '../../packages/api/src/generated'] = Bun.argv.slice(2)
const source = await Bun.file(input).text()
const document = JSON.parse(source)
const contractId = document?.info?.version
if (typeof contractId !== 'string' || !contractId) {
  throw new Error('OpenAPI info.version must contain the API contract ID.')
}

const temporary = await mkdtemp(join(tmpdir(), 'orbit-api-'))
const temporaryInput = join(temporary, 'openapi.json')
await Bun.write(temporaryInput, source)
await rm(output, { recursive: true, force: true })
await mkdir(output, { recursive: true })
try {
  await createClient({
    input: temporaryInput,
    output: { path: output, entryFile: false },
    plugins: [...defaultPlugins, '@hey-api/client-fetch'],
    logs: { level: 'silent', file: false },
  })
} finally {
  await rm(temporary, { recursive: true, force: true })
}

const rolePermissions = document['x-role-permissions']
if (typeof rolePermissions !== 'object' || !rolePermissions) {
  throw new Error('OpenAPI x-role-permissions must contain the role table.')
}

await Bun.write(join(output, 'openapi.json'), source)
await Bun.write(
  join(output, 'rolePermissions.ts'),
  `// Generated from OpenAPI x-role-permissions. Do not edit.\n// For test fixtures and mocks only: the app reads \`workspace.permissions\` from the server.\nimport type { Permission, WorkspaceRole } from './types.gen'\n\nexport const ROLE_PERMISSIONS: Record<WorkspaceRole, Permission[]> = ${JSON.stringify(rolePermissions, null, 2)}\n`,
)
await Bun.write(
  join(output, 'contract.ts'),
  `// Generated from OpenAPI info.version. Do not edit.\nexport const CONTRACT_ID = ${JSON.stringify(contractId)} as const\n`,
)
