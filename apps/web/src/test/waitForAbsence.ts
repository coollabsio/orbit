import { waitFor } from '@testing-library/react'

/**
 * Waits until `query()` returns null. Use this instead of `waitFor(() => expect(node).toBeNull())`: every failed poll
 * makes Bun format the whole happy-dom node for the error message, which takes seconds late in a full test run.
 */
export function waitForAbsence(query: () => Element | null) {
  return waitFor(() => {
    if (query() !== null) throw new Error('The element is still in the document.')
  })
}
