import { describe, expect, mock, test } from 'bun:test'
import { mentionableMembers } from './mentionable'
import { commentEditorSchema, mentionItems } from './mentions'

const members = [
  { id: 'me', name: 'Test User', handle: 'test' },
  { id: 'ann', name: 'Ann Lee', handle: 'ann' },
  { id: 'bob', name: 'Bob Stone', handle: 'bstone' },
]

describe('mention picker', () => {
  test('offers every other member on teamspace pages', () => {
    expect(mentionableMembers(members, { teamspace_id: 'ts' }, 'me').map((member) => member.id)).toEqual(['ann', 'bob'])
  })

  test('offers nobody on private pages (only the owner can see them)', () => {
    expect(mentionableMembers(members, { teamspace_id: null }, 'me')).toEqual([])
  })

  test('filters by name or handle, case-insensitively, and inserts the picked member', () => {
    const insert = mock(() => {})
    const items = mentionItems(members, 'STO', insert)
    expect(items.map((item) => item.title)).toEqual(['Bob Stone'])
    expect(mentionItems(members, 'bst', insert).map((item) => item.title)).toEqual(['Bob Stone'])
    expect(mentionItems(members, '', insert)).toHaveLength(3)
    items[0].onItemClick()
    expect(insert).toHaveBeenCalledWith(members[2])
  })

  test('caps the list at eight entries', () => {
    const many = Array.from({ length: 12 }, (_, index) => ({ id: `u${index}`, name: `User ${index}` }))
    expect(mentionItems(many, 'user', () => {})).toHaveLength(8)
  })

  test('the comment editor schema has paragraphs and the mention node', () => {
    expect(Object.keys(commentEditorSchema.blockSchema)).toEqual(['paragraph'])
    expect(commentEditorSchema.inlineContentSchema.mention.propSchema).toEqual({ userId: { default: '' }, name: { default: '' } })
    expect('textColor' in commentEditorSchema.styleSchema).toBe(false)
  })
})
