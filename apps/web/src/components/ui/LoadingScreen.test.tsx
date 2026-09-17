import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { LoadingScreen } from './LoadingScreen'

test('shows one general loading indicator without staged loading copy', () => {
  const { container, getByRole } = render(<LoadingScreen />)

  expect(getByRole('status', { name: 'Loading' })).toBeTruthy()
  expect(container.textContent).toBe('')
})
