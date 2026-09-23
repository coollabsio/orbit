import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './select'

test('select fields use the shared input style and allow compact widths', () => {
  const view = render(
    <>
      <Select items={[{ value: 'one', label: 'One' }]} defaultValue="one">
        <SelectTrigger aria-label="Standard"><SelectValue /></SelectTrigger>
        <SelectContent><SelectItem value="one">One</SelectItem></SelectContent>
      </Select>
      <Select items={[{ value: 'one', label: 'One' }]} defaultValue="one">
        <SelectTrigger aria-label="Compact" size="sm" className="w-16"><SelectValue /></SelectTrigger>
        <SelectContent><SelectItem value="one">One</SelectItem></SelectContent>
      </Select>
    </>,
  )

  const standard = view.getByRole('combobox', { name: 'Standard' })
  const compact = view.getByRole('combobox', { name: 'Compact' })
  for (const trigger of [standard, compact]) {
    for (const style of ['rounded-lg', 'border-input', 'bg-transparent', 'dark:bg-input/30']) {
      expect(trigger.classList.contains(style)).toBe(true)
    }
  }
  expect(standard.classList.contains('w-full')).toBe(true)
  expect(compact.classList.contains('w-16')).toBe(true)
  expect(compact.getAttribute('data-size')).toBe('sm')
})
