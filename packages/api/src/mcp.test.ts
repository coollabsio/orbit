import { describe, expect, test } from 'bun:test'
import { mcpClientCommands, mcpUrl } from './mcp'

describe('mcpUrl', () => {
  test('appends /mcp to the origin, without a double slash', () => {
    expect(mcpUrl('https://orbit.example.com')).toBe('https://orbit.example.com/mcp')
    expect(mcpUrl('https://orbit.example.com/')).toBe('https://orbit.example.com/mcp')
  })
})

describe('mcpClientCommands', () => {
  test('puts the endpoint into every client command', () => {
    const commands = mcpClientCommands('http://127.0.0.1:18888/mcp')
    expect(commands.map((entry) => entry.client)).toEqual(['Claude', 'Codex', 'Grok'])
    expect(commands[0].command).toBe('claude mcp add --scope user --transport http orbit http://127.0.0.1:18888/mcp')
    expect(commands[1].command).toBe('codex mcp add orbit --url http://127.0.0.1:18888/mcp\ncodex mcp login orbit')
  })
})
