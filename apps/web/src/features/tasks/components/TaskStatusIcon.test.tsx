import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { TaskStatusIcon } from './TaskStatusIcon'

test('duplicate glyph is a filled circle in the status color with a card-colored equals sign', () => {
  const view = render(<TaskStatusIcon status={{ category: 'duplicate', color: '#8b8f98' }} />)
  const icon = view.getByLabelText('Duplicate')
  expect(icon.querySelector('circle')?.getAttribute('fill')).toBe('#8b8f98')
  const strokes = icon.querySelector('path')!
  expect(strokes.getAttribute('d')).toBe('M4.5 5.6 H9.5 M4.5 8.4 H9.5')
  expect(strokes.getAttribute('stroke')).toBe('var(--card)')
})
