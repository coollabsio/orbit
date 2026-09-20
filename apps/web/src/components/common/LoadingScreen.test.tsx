import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { LoadingScreen } from './LoadingScreen'

test('shows one general loading indicator without staged loading copy', () => {
  const { container, getByRole } = render(<LoadingScreen />)

  const indicator = getByRole('status', { name: 'Loading' })
  expect(indicator.getAttribute('data-slot')).toBe('spinner')
  expect(container.textContent).toBe('')
})
